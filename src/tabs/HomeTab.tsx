import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { LayoutGroup, motion } from "motion/react";
import { Eye, EyeOff, Monitor, MonitorOff, SlidersHorizontal } from "lucide-react";
import clsx from "clsx";
import { usePrefs } from "../lib/prefs";
import { useHoldToLift } from "../lib/hold";
import { useDesktopWidgets } from "../lib/desktop";
import { WIDGETS, type WidgetDef } from "../widgets/registry";

/** Must match the `gap-3` of the masonry below. */
const GAP = 12;

const BY_ID = new Map(WIDGETS.map((w) => [w.id, w]));

/**
 * Saved order first (skipping removed widgets), then any widgets added since.
 * Pinned apps used to sit above all widgets, so they go first when missing.
 */
function useWidgetOrder() {
  const widgetOrder = usePrefs((s) => s.widgetOrder);
  return useMemo(() => {
    const known = widgetOrder.filter((id) => BY_ID.has(id));
    const added = WIDGETS.map((w) => w.id).filter((id) => !known.includes(id));
    const first = added.includes("pinned") ? ["pinned"] : [];
    return [...first, ...known, ...added.filter((id) => id !== "pinned")];
  }, [widgetOrder]);
}

/** How many columns of `width`-wide widgets the masonry fits into the container. */
function useColumnCount(ref: RefObject<HTMLDivElement | null>, width: number) {
  const [count, setCount] = useState(1);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setCount(Math.max(1, Math.floor((el.clientWidth + GAP) / (width + GAP))));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, width]);
  return count;
}

function shortest(cols: string[][]) {
  return cols.reduce((a, b) => (b.length < a.length ? b : a));
}

/** Changes the number of columns, keeping widgets in place where possible. */
function reshape(cols: string[][], count: number) {
  if (cols.length === count) return cols;
  if (cols.length > count) return [...cols.slice(0, count - 1), cols.slice(count - 1).flat()];
  return [...cols, ...Array.from({ length: count - cols.length }, (): string[] => [])];
}

/**
 * The hand-made arrangement for this many columns, if there is one: removed
 * widgets dropped, widgets added since go to the shortest column.
 */
function useArrangement(count: number, order: string[]) {
  const saved = usePrefs((s) => s.widgetColumns[count]);
  return useMemo(() => {
    if (!saved) return null;
    const seen = new Set<string>();
    const cols = reshape(saved, count).map((col) =>
      col.filter((id) => BY_ID.has(id) && !seen.has(id) && seen.add(id)),
    );
    for (const id of order) if (!seen.has(id)) shortest(cols).push(id);
    return cols;
  }, [saved, count, order]);
}

/** Reads the columns the masonry actually put the widgets in; hidden ones go to the shortest column. */
function measureColumns(container: HTMLElement, count: number, order: string[]) {
  const box = container.getBoundingClientRect();
  const colWidth = (box.width - GAP * (count - 1)) / count;
  const cols: string[][] = Array.from({ length: count }, () => []);
  const placed = [...container.querySelectorAll<HTMLElement>("[data-widget]")]
    .map((el) => ({ id: el.dataset.widget!, rect: el.getBoundingClientRect() }))
    .sort((a, b) => a.rect.top - b.rect.top);
  for (const { id, rect } of placed) {
    const i = Math.min(count - 1, Math.max(0, Math.floor((rect.left - box.left + 1) / (colWidth + GAP))));
    cols[i].push(id);
  }
  const seen = new Set(cols.flat());
  for (const id of order) if (!seen.has(id)) shortest(cols).push(id);
  return cols;
}

function EyeButton({ visible, onClick }: { visible: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      onPointerDown={(e) => e.stopPropagation()}
      title={visible ? "Скрыть" : "Показать"}
      className="grid size-7 place-items-center rounded-lg border border-stroke bg-popover text-fg-muted shadow-md hover:text-fg"
    >
      {visible ? <Eye className="size-4" /> : <EyeOff className="size-4" />}
    </button>
  );
}

/** Puts the widget on the desktop (in a window of its own) or takes it off. */
function DesktopButton({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      onPointerDown={(e) => e.stopPropagation()}
      title={on ? "Убрать с рабочего стола" : "На рабочий стол"}
      className={clsx(
        "grid size-7 place-items-center rounded-lg border border-stroke bg-popover shadow-md",
        on ? "text-accent" : "text-fg-muted hover:text-fg",
      )}
    >
      {on ? <MonitorOff className="size-4" /> : <Monitor className="size-4" />}
    </button>
  );
}

interface Drag {
  id: string;
  /** Grab point inside the widget. */
  dx: number;
  dy: number;
  width: number;
  x: number;
  y: number;
}

/** The widget following the cursor. Moves itself, so the editor doesn't re-render on every pointer move. */
function DragGhost({ drag }: { drag: Drag }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (ref.current) ref.current.style.transform = `translate(${e.clientX - drag.dx}px, ${e.clientY - drag.dy}px)`;
    };
    window.addEventListener("pointermove", move);
    return () => window.removeEventListener("pointermove", move);
  }, [drag]);

  const Widget = BY_ID.get(drag.id)!.component;
  return createPortal(
    <div
      ref={ref}
      className="pointer-events-none fixed top-0 left-0 z-50"
      style={{ width: drag.width, transform: `translate(${drag.x - drag.dx}px, ${drag.y - drag.dy}px)` }}
    >
      <div className="scale-[1.02] rotate-1 rounded-2xl bg-popover opacity-95 shadow-2xl shadow-black/40">
        <Widget />
      </div>
    </div>,
    document.body,
  );
}

/** Where a drop at (x, y) would put `id`: the nearest column, below every widget whose middle is above y. */
function dropTarget(
  cols: string[][],
  id: string,
  x: number,
  y: number,
  colEls: (HTMLDivElement | null)[],
  itemEls: Map<string, HTMLDivElement>,
) {
  let col = 0;
  let best = Infinity;
  cols.forEach((_, i) => {
    const r = colEls[i]?.getBoundingClientRect();
    if (!r) return;
    const d = x < r.left ? r.left - x : x > r.right ? x - r.right : 0;
    if (d < best) {
      best = d;
      col = i;
    }
  });
  const top = colEls[col]?.getBoundingClientRect().top ?? 0;
  // offsetTop ignores the layout animation's transforms, so targets don't jitter mid-animation.
  const index = cols[col].filter((other) => {
    const el = itemEls.get(other);
    return other !== id && el?.isConnected && top + el.offsetTop + el.offsetHeight / 2 < y;
  }).length;
  return { col, index };
}

function moveTo(cols: string[][], id: string, { col, index }: { col: number; index: number }) {
  const from = cols.findIndex((c) => c.includes(id));
  if (from === col && cols[col].indexOf(id) === index) return cols;
  const next = cols.map((c) => c.filter((other) => other !== id));
  next[col].splice(index, 0, id);
  return next;
}

/**
 * Live widgets in their real columns: drag anywhere on a widget to move it, eye to hide.
 *
 * With `liveDrag` it's the press-and-hold move on the normal home tab instead:
 * no frame, buttons or hidden widgets, already dragging, and the arrangement
 * is saved the moment the mouse is released.
 */
function LayoutEditor({
  initial,
  count,
  onDone,
  liveDrag,
}: {
  initial: string[][];
  count: number;
  onDone: (cols: string[][] | null, hidden: string[]) => void;
  liveDrag?: Drag;
}) {
  const live = liveDrag != null;
  const [cols, setCols] = useState(initial);
  const [hidden, setHidden] = useState(() => new Set(usePrefs.getState().hiddenWidgets));
  const [drag, setDrag] = useState<Drag | null>(liveDrag ?? null);
  const desktop = useDesktopWidgets();
  const colEls = useRef<(HTMLDivElement | null)[]>([]);
  const itemEls = useRef(new Map<string, HTMLDivElement>());
  // The drop handler reads these after the last re-render.
  const latest = useRef({ cols, hidden, onDone });
  latest.current = { cols, hidden, onDone };

  // The panel was resized while editing.
  useEffect(() => setCols((prev) => reshape(prev, count)), [count]);

  const dragId = drag?.id;
  useEffect(() => {
    if (!dragId) return;
    const pointer = { x: 0, y: 0 };
    const place = () =>
      setCols((prev) => moveTo(prev, dragId, dropTarget(prev, dragId, pointer.x, pointer.y, colEls.current, itemEls.current)));
    const onMove = (e: PointerEvent) => {
      pointer.x = e.clientX;
      pointer.y = e.clientY;
      place();
    };
    const onUp = () => {
      setDrag(null);
      if (!live) return;
      // The release would also click whatever button is under the cursor; swallow that click.
      const swallow = (e: MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 100);
      const { cols, hidden, onDone } = latest.current;
      onDone(cols, [...hidden]);
    };

    // Scroll the tab while the cursor is near its top or bottom edge.
    const scroller = colEls.current[0]?.closest(".scroll-area");
    let frame = requestAnimationFrame(function tick() {
      const r = scroller?.getBoundingClientRect();
      if (scroller && r && pointer.y) {
        const edge = 56;
        const over = pointer.y < r.top + edge ? pointer.y - r.top - edge : pointer.y > r.bottom - edge ? pointer.y - r.bottom + edge : 0;
        if (over) {
          scroller.scrollTop += over / 4;
          place();
        }
      }
      frame = requestAnimationFrame(tick);
    });

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
  }, [dragId]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (id: string) =>
    setHidden((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const startDrag = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0) return;
    const r = itemEls.current.get(id)?.getBoundingClientRect();
    if (!r) return;
    e.preventDefault();
    setDrag({ id, dx: e.clientX - r.left, dy: e.clientY - r.top, width: r.width, x: e.clientX, y: e.clientY });
  };

  const button = "rounded-lg px-3 py-1 text-[12.5px] font-medium";
  return (
    <div className="flex flex-col gap-3">
      <div className={clsx("flex items-center gap-2 px-1", live && "hidden")}>
        <span className="flex-1 text-[12px] text-fg-subtle">
          Перетаскивайте виджеты{count > 1 && " между колонками"}, глаз — скрыть, монитор — на рабочий стол
        </span>
        <button
          onClick={() => onDone(null, [...hidden])}
          title="Забыть ручную расстановку: виджеты сами распределятся по колонкам"
          className={clsx(button, "text-fg-muted hover:bg-ink/10 hover:text-fg")}
        >
          Авто
        </button>
        <button
          onClick={() => onDone(cols, [...hidden])}
          className={clsx(button, "bg-accent text-on-accent hover:bg-accent/90")}
        >
          Готово
        </button>
      </div>

      <LayoutGroup>
        <div className={clsx("flex items-start gap-3", drag && "cursor-grabbing select-none")}>
          {cols.map((col, i) => (
            <div
              key={i}
              ref={(el) => {
                colEls.current[i] = el;
              }}
              className={clsx(
                "relative flex min-w-0 flex-1 flex-col gap-3 rounded-2xl",
                // In the live move an empty column still needs room to drop into.
                live ? "min-h-24" : "min-h-32",
                col.length === 0 && "border-2 border-dashed border-stroke",
              )}
            >
              {col.map((id) => {
                const { component: Widget, title } = BY_ID.get(id) as WidgetDef;
                const dragging = drag?.id === id;
                const visible = !hidden.has(id);
                if (live && !visible) return null;
                return (
                  <motion.div
                    key={id}
                    layoutId={`widget-${id}`}
                    layout="position"
                    transition={{ type: "spring", stiffness: 520, damping: 42 }}
                    ref={(el: HTMLDivElement | null) => {
                      if (el) itemEls.current.set(id, el);
                    }}
                    className="relative"
                  >
                    {/* A widget with nothing to show (say, VPN without clients) still needs something to grab. */}
                    <div
                      data-empty={`${title} — сейчас пусто`}
                      className={clsx(
                        "empty:flex empty:h-14 empty:items-center empty:rounded-2xl empty:border empty:border-dashed empty:border-stroke empty:px-3.5 empty:text-[12px] empty:text-fg-subtle empty:before:content-[attr(data-empty)]",
                        dragging && "invisible",
                        !visible && "opacity-35 grayscale",
                      )}
                    >
                      <Widget />
                    </div>
                    {dragging && (
                      <div className="absolute inset-0 rounded-2xl border-2 border-dashed border-accent/70 bg-accent/8" />
                    )}
                    {/* Blocks the widget's own buttons while editing (and while a live move is on). */}
                    {(!live || drag) && (
                      <div
                        onPointerDown={(e) => startDrag(e, id)}
                        className={clsx(
                          "absolute inset-0 touch-none rounded-2xl",
                          !live && "ring-1 ring-accent/35 ring-inset",
                          drag ? "cursor-grabbing" : "cursor-grab hover:bg-accent/5",
                        )}
                      />
                    )}
                    {!dragging && !live && (
                      <div className="absolute top-2 right-2 flex gap-1">
                        <DesktopButton on={desktop.onDesktop.includes(id)} onClick={() => desktop.set(id, !desktop.onDesktop.includes(id))} />
                        <EyeButton visible={visible} onClick={() => toggle(id)} />
                      </div>
                    )}
                  </motion.div>
                );
              })}
            </div>
          ))}
        </div>
      </LayoutGroup>

      {drag && <DragGhost drag={drag} />}
    </div>
  );
}

function useNow() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(id);
  }, []);
  return now;
}

export function HomeTab() {
  const now = useNow();
  const containerRef = useRef<HTMLDivElement>(null);
  const widgetWidth = usePrefs((s) => s.widgetWidth);
  const count = useColumnCount(containerRef, widgetWidth);
  const order = useWidgetOrder();
  const arranged = useArrangement(count, order);
  const hidden = usePrefs((s) => s.hiddenWidgets);
  const setWidgetLayout = usePrefs((s) => s.setWidgetLayout);
  const [editing, setEditing] = useState<string[][] | null>(null);
  // A widget lifted by press-and-hold, with the columns it moves between.
  const [moving, setMoving] = useState<{ cols: string[][]; drag: Drag } | null>(null);
  const time = now.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  const date = now.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" });
  const visible = (id: string) => !hidden.includes(id);

  const startEditing = () => {
    const el = containerRef.current;
    setEditing(arranged ?? (el ? measureColumns(el, count, order) : [order]));
  };
  const finishEditing = (cols: string[][] | null, hiddenIds: string[]) => {
    setWidgetLayout(cols ? cols.flat() : order, hiddenIds, cols);
    setEditing(null);
  };
  const finishMoving = (cols: string[][] | null, hiddenIds: string[]) => {
    setWidgetLayout(cols ? cols.flat() : order, hiddenIds, cols);
    setMoving(null);
  };

  const hold = useHoldToLift((id: string, widget, start) => {
    const el = containerRef.current;
    const r = widget.getBoundingClientRect();
    const drag = { id, dx: start.x - r.left, dy: start.y - r.top, width: r.width, x: start.x, y: start.y };
    setMoving({ cols: arranged ?? (el ? measureColumns(el, count, order) : [order]), drag });
  });

  const slot = (id: string) => {
    const Widget = BY_ID.get(id)!.component;
    return (
      <div
        key={id}
        data-widget={id}
        onPointerDown={(e) => hold(e, id)}
        className="mb-3 break-inside-avoid"
      >
        <Widget />
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-3 pb-2">
      <div className="flex items-end justify-between px-1 pt-1 pb-2">
        <div>
          <div className="font-display text-[52px] leading-none font-semibold tracking-tight tabular-nums">{time}</div>
          <div className="mt-1.5 text-[14px] text-fg-muted first-letter:uppercase">{date}</div>
        </div>
        {!editing && !moving && (
          <button
            onClick={startEditing}
            title="Расставить виджеты"
            className="grid size-8 place-items-center rounded-lg text-fg-subtle hover:bg-ink/8 hover:text-fg"
          >
            <SlidersHorizontal className="size-4" />
          </button>
        )}
      </div>

      <div ref={containerRef}>
        {editing ? (
          <LayoutEditor initial={editing} count={count} onDone={finishEditing} />
        ) : moving ? (
          <LayoutEditor initial={moving.cols} count={count} onDone={finishMoving} liveDrag={moving.drag} />
        ) : arranged ? (
          <div className="flex items-start gap-3">
            {arranged.map((col, i) => (
              <div key={i} className="flex min-w-0 flex-1 flex-col">
                {col.filter(visible).map(slot)}
              </div>
            ))}
          </div>
        ) : (
          // Masonry: a wider panel gets more widget columns instead of wider widgets.
          <div className="gap-3" style={{ columnWidth: widgetWidth }}>{order.filter(visible).map(slot)}</div>
        )}
      </div>
    </div>
  );
}
