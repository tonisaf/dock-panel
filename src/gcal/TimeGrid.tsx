import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import { CircleCheck, Circle } from "lucide-react";
import clsx from "clsx";
import { addDays, bounds, DAY_MS, hhmm, onColor, sameDay, useGcalActions, ymd, type GEvent, type GTask } from "./api";
import type { CardTarget } from "./EventCard";

const HOUR = 48;
const GUTTER = 56;
const SNAP_MIN = 15;
/** A click (no drag) on the grid creates an event this long. */
const DEFAULT_MIN = 60;
const DRAG_THRESHOLD = 4;

interface Placed {
  event: GEvent;
  start: Date;
  end: Date;
  top: number;
  height: number;
  col: number;
  cols: number;
}

/** Timed events of one day, cut to the day and spread into side-by-side columns where they overlap. */
function layoutDay(day: Date, events: GEvent[]): Placed[] {
  const dayStart = day.getTime();
  const dayEnd = dayStart + DAY_MS;
  const items = events
    .filter((e) => !e.allDay)
    .map((e) => ({ e, ...bounds(e) }))
    .filter(({ start, end }) => start.getTime() < dayEnd && end.getTime() > dayStart)
    .map(({ e, start, end }) => {
      const s = Math.max(start.getTime(), dayStart);
      const f = Math.min(end.getTime(), dayEnd);
      return { e, start, end, s, f };
    })
    .sort((a, b) => a.s - b.s || b.f - a.f);

  const placed: Placed[] = [];
  let cluster: { item: (typeof items)[number]; col: number }[] = [];
  let clusterEnd = -Infinity;
  const flush = () => {
    const cols = Math.max(1, ...cluster.map((c) => c.col + 1));
    for (const { item, col } of cluster) {
      placed.push({
        event: item.e,
        start: item.start,
        end: item.end,
        top: ((item.s - dayStart) / 3_600_000) * HOUR,
        height: Math.max(((item.f - item.s) / 3_600_000) * HOUR, 18),
        col,
        cols,
      });
    }
    cluster = [];
  };
  for (const item of items) {
    if (item.s >= clusterEnd && cluster.length) flush();
    // The first column whose last event has ended.
    const taken = new Set(cluster.filter((c) => c.item.f > item.s).map((c) => c.col));
    let col = 0;
    while (taken.has(col)) col++;
    cluster.push({ item, col });
    clusterEnd = Math.max(clusterEnd, item.f);
  }
  flush();
  return placed;
}

interface Drag {
  kind: "move" | "resize" | "create";
  event?: GEvent;
  pointerX: number;
  pointerY: number;
  /** Minutes from midnight of `day` where the drag began. */
  originMin: number;
  originDay: number;
  start: Date;
  end: Date;
  moved: boolean;
  /** Live preview. */
  day: number;
  previewStart: Date;
  previewEnd: Date;
}

function TasksChip({ tasks }: { tasks: GTask[] }) {
  const { completeTask } = useGcalActions();
  const [open, setOpen] = useState<{ x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  if (tasks.length === 0) return null;
  const label = tasks.length === 1 ? tasks[0].title : `${tasks.length} ${plural(tasks.length)}`;
  return (
    <>
      <button
        onClick={(e) => setOpen({ x: e.clientX, y: e.clientY })}
        className="flex w-full items-center gap-1 truncate rounded-md bg-accent/15 px-1.5 py-0.5 text-left text-[11.5px] font-medium text-fg hover:bg-accent/25"
      >
        <CircleCheck className="size-3.5 shrink-0 text-accent" />
        <span className="truncate">{label}</span>
      </button>
      {open &&
        createPortal(
          <div
            ref={ref}
            style={{ left: Math.min(open.x, window.innerWidth - 328), top: open.y + 8 }}
            className="fixed z-50 flex max-h-80 w-80 flex-col gap-0.5 overflow-y-auto rounded-xl border border-ink/10 bg-popover p-1.5 text-fg shadow-2xl shadow-black/40"
          >
            {tasks.map((t) => (
              <button
                key={t.id}
                onClick={() => completeTask(t).catch(console.error)}
                title="Отметить выполненной"
                className="group flex items-start gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-ink/8"
              >
                <Circle className="mt-0.5 size-4 shrink-0 text-fg-subtle group-hover:hidden" />
                <CircleCheck className="mt-0.5 hidden size-4 shrink-0 text-accent group-hover:block" />
                <span className="min-w-0">
                  <span className="block text-[13px]">{t.title}</span>
                  <span className="block text-[11px] text-fg-subtle">
                    {t.list}
                    {t.due && ` · ${new Date(`${t.due}T00:00`).toLocaleDateString("ru-RU", { day: "numeric", month: "short" })}`}
                  </span>
                </span>
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

function plural(n: number) {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return "невыполненная задача";
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return "невыполненные задачи";
  return "невыполненных задач";
}

const WEEKDAY = ["вс", "пн", "вт", "ср", "чт", "пт", "сб"];

export function TimeGrid({
  days,
  events,
  tasks,
  onOpen,
  onPickDay,
  onError,
}: {
  days: Date[];
  events: GEvent[];
  tasks: GTask[];
  onOpen: (target: CardTarget) => void;
  onPickDay: (day: Date) => void;
  onError: (message: string) => void;
}) {
  const { retime } = useGcalActions();
  const scroller = useRef<HTMLDivElement>(null);
  const columns = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(id);
  }, []);

  const today = days.findIndex((d) => sameDay(d, now));
  // Open around now (or the morning), like Google does.
  useLayoutEffect(() => {
    const hour = today >= 0 ? Math.max(0, now.getHours() + now.getMinutes() / 60 - 2) : 7;
    scroller.current?.scrollTo({ top: hour * HOUR });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the range changes
  }, [days[0]?.getTime()]);

  const placed = useMemo(() => days.map((d) => layoutDay(d, events)), [days, events]);
  const allDay = useMemo(
    () =>
      days.map((d) =>
        events.filter((e) => {
          if (!e.allDay) return false;
          const { start, end } = bounds(e);
          return start <= d && end > d;
        }),
      ),
    [days, events],
  );
  const tasksByDay = useMemo(() => {
    const todayKey = ymd(now);
    return days.map((d) => {
      const key = ymd(d);
      // Overdue tasks gather on today, as in Google Calendar.
      return tasks.filter((t) => t.due && (t.due === key || (key === todayKey && t.due < todayKey)));
    });
  }, [days, tasks, now]);

  /** Grid position under the pointer: day index and snapped minutes. */
  const at = (x: number, y: number) => {
    const rect = columns.current!.getBoundingClientRect();
    const day = Math.min(days.length - 1, Math.max(0, Math.floor(((x - rect.left) / rect.width) * days.length)));
    const min = Math.min(24 * 60, Math.max(0, Math.round((((y - rect.top) / HOUR) * 60) / SNAP_MIN) * SNAP_MIN));
    return { day, min };
  };
  const dateAt = (day: number, min: number) => new Date(days[day].getTime() + min * 60_000);

  const begin = (e: ReactPointerEvent, kind: Drag["kind"], event?: GEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const { day, min } = at(e.clientX, e.clientY);
    const b = event ? bounds(event) : { start: dateAt(day, min), end: dateAt(day, min + DEFAULT_MIN) };
    setDrag({
      kind,
      event,
      pointerX: e.clientX,
      pointerY: e.clientY,
      originMin: min,
      originDay: day,
      start: b.start,
      end: b.end,
      moved: false,
      day,
      previewStart: b.start,
      previewEnd: b.end,
    });
  };

  const move = (e: ReactPointerEvent) => {
    if (!drag) return;
    const moved = drag.moved || Math.hypot(e.clientX - drag.pointerX, e.clientY - drag.pointerY) > DRAG_THRESHOLD;
    const { day, min } = at(e.clientX, e.clientY);
    const deltaMin = min - drag.originMin + (day - drag.originDay) * 24 * 60;
    let previewStart = drag.start;
    let previewEnd = drag.end;
    if (drag.kind === "move") {
      previewStart = new Date(drag.start.getTime() + deltaMin * 60_000);
      previewEnd = new Date(drag.end.getTime() + deltaMin * 60_000);
    } else if (drag.kind === "resize") {
      previewEnd = new Date(Math.max(drag.start.getTime() + SNAP_MIN * 60_000, drag.end.getTime() + deltaMin * 60_000));
    } else {
      // Creating: from the press point to the pointer, either direction, within the day.
      const a = dateAt(drag.originDay, drag.originMin);
      const b = dateAt(drag.originDay, min === drag.originMin ? min + SNAP_MIN : min);
      previewStart = a < b ? a : b;
      previewEnd = a < b ? b : a;
    }
    setDrag({ ...drag, moved, day, previewStart, previewEnd });
  };

  const end = (e: ReactPointerEvent) => {
    if (!drag) return;
    const d = drag;
    setDrag(null);
    if (d.kind === "create") {
      const start = d.moved ? d.previewStart : d.start;
      const finish = d.moved ? d.previewEnd : d.end;
      onOpen({ kind: "new", start, end: finish, allDay: false, x: e.clientX, y: e.clientY });
      return;
    }
    if (!d.moved) {
      onOpen({ kind: "event", event: d.event!, x: e.clientX, y: e.clientY });
      return;
    }
    if (d.previewStart.getTime() !== d.start.getTime() || d.previewEnd.getTime() !== d.end.getTime()) {
      retime(d.event!, d.previewStart, d.previewEnd).catch((err) => onError(String(err)));
    }
  };

  const gmt = (() => {
    const off = -now.getTimezoneOffset() / 60;
    return `GMT${off >= 0 ? "+" : "-"}${String(Math.abs(off)).padStart(2, "0")}`;
  })();
  const dragging = (ev: GEvent) =>
    drag?.event && drag.moved && drag.event.calendarId === ev.calendarId && drag.event.id === ev.id;

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Day headers */}
      <div className="flex shrink-0 pr-2" style={{ paddingLeft: GUTTER }}>
        {days.map((d, i) => (
          <div key={i} className="flex flex-1 flex-col items-center gap-0.5 pb-1">
            <span className={clsx("text-[11px] font-medium uppercase", i === today ? "text-accent" : "text-fg-muted")}>
              {WEEKDAY[d.getDay()]}
            </span>
            <button
              onClick={() => onPickDay(d)}
              className={clsx(
                "grid size-10 place-items-center rounded-full text-[22px] tabular-nums transition-colors",
                i === today ? "bg-accent text-on-accent" : "hover:bg-ink/8",
              )}
            >
              {d.getDate()}
            </button>
          </div>
        ))}
      </div>

      {/* All-day events and tasks */}
      <div className="flex shrink-0 border-b border-stroke pr-2">
        <div className="shrink-0 pt-1 pr-2 text-right text-[10.5px] text-fg-subtle" style={{ width: GUTTER }}>
          {gmt}
        </div>
        {days.map((d, i) => (
          <div
            key={i}
            onDoubleClick={(e) => onOpen({ kind: "new", start: d, end: addDays(d, 1), allDay: true, x: e.clientX, y: e.clientY })}
            className="flex min-h-7 min-w-0 flex-1 flex-col gap-0.5 border-l border-stroke px-0.5 py-0.5"
          >
            <TasksChip tasks={tasksByDay[i]} />
            {allDay[i].map((ev) => (
              <button
                key={`${ev.calendarId}/${ev.id}`}
                onClick={(e) => onOpen({ kind: "event", event: ev, x: e.clientX, y: e.clientY })}
                className="truncate rounded-md px-1.5 py-0.5 text-left text-[11.5px] font-medium"
                style={{ background: ev.color, color: onColor(ev.color) }}
              >
                {ev.title}
              </button>
            ))}
          </div>
        ))}
      </div>

      {/* Hours */}
      <div ref={scroller} className="scroll-area relative min-h-0 flex-1">
        <div className="relative flex" style={{ height: 24 * HOUR }}>
          <div className="relative shrink-0" style={{ width: GUTTER }}>
            {Array.from({ length: 23 }, (_, h) => (
              <span
                key={h}
                className="absolute right-2 -translate-y-1/2 text-[10.5px] text-fg-subtle tabular-nums"
                style={{ top: (h + 1) * HOUR }}
              >
                {String(h + 1).padStart(2, "0")}:00
              </span>
            ))}
          </div>
          <div
            ref={columns}
            className="relative flex flex-1"
            onPointerMove={move}
            onPointerUp={end}
            onPointerCancel={() => setDrag(null)}
          >
            {/* Hour lines */}
            {Array.from({ length: 24 }, (_, h) => (
              <div key={h} className="pointer-events-none absolute inset-x-0 border-t border-stroke" style={{ top: h * HOUR }} />
            ))}
            {days.map((d, i) => (
              <div
                key={i}
                className="relative flex-1 border-l border-stroke"
                onPointerDown={(e) => begin(e, "create")}
              >
                {placed[i].map((p) => {
                  const ev = p.event;
                  const hidden = dragging(ev);
                  const short = p.height < 36;
                  return (
                    <div
                      key={`${ev.calendarId}/${ev.id}`}
                      onPointerDown={(e) => (ev.editable ? begin(e, "move", ev) : e.stopPropagation())}
                      onClick={(e) => !ev.editable && onOpen({ kind: "event", event: ev, x: e.clientX, y: e.clientY })}
                      className={clsx(
                        "absolute overflow-hidden rounded-md px-1.5 text-[11.5px] leading-tight shadow-sm ring-1 ring-black/5 select-none",
                        ev.editable ? "cursor-grab active:cursor-grabbing" : "cursor-pointer",
                        hidden && "opacity-30",
                        short ? "py-0" : "py-1",
                      )}
                      style={{
                        top: p.top + 1,
                        height: p.height - 2,
                        left: `calc(${(p.col / p.cols) * 100}% + 1px)`,
                        width: `calc(${100 / p.cols}% - 3px)`,
                        background: ev.color,
                        color: onColor(ev.color),
                      }}
                    >
                      <div className={clsx("font-medium", short ? "truncate" : "line-clamp-2")}>
                        {ev.title}
                        {short && <span className="font-normal opacity-85">, {hhmm(p.start)}</span>}
                      </div>
                      {!short && (
                        <div className="truncate opacity-85">
                          {hhmm(p.start)}–{hhmm(p.end)}
                        </div>
                      )}
                      {ev.editable && (
                        <div
                          onPointerDown={(e) => begin(e, "resize", ev)}
                          className="absolute inset-x-0 bottom-0 h-1.5 cursor-ns-resize"
                        />
                      )}
                    </div>
                  );
                })}

                {/* Where a dragged or new event would land */}
                {drag?.moved && drag.kind !== "create" && drag.day === i && (
                  <Ghost drag={drag} day={d} color={drag.event?.color} />
                )}
                {drag?.moved && drag.kind === "create" && drag.originDay === i && <Ghost drag={drag} day={d} />}

                {i === today && (
                  <div
                    className="pointer-events-none absolute inset-x-0 z-10 border-t-2 border-[#ea4335]"
                    style={{ top: ((now.getHours() * 60 + now.getMinutes()) / 60) * HOUR }}
                  >
                    <span className="absolute -top-[6px] -left-[6px] size-2.5 rounded-full bg-[#ea4335]" />
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function Ghost({ drag, day, color }: { drag: Drag; day: Date; color?: string }) {
  const top = ((drag.previewStart.getTime() - day.getTime()) / 3_600_000) * HOUR;
  const height = Math.max(((drag.previewEnd.getTime() - drag.previewStart.getTime()) / 3_600_000) * HOUR, 12);
  const bg = color ?? "var(--color-accent)";
  return (
    <div
      className="pointer-events-none absolute inset-x-0.5 z-20 rounded-md px-1.5 py-1 text-[11.5px] font-medium shadow-lg"
      style={{ top, height, background: bg, color: color ? onColor(color) : "var(--color-on-accent)", opacity: 0.9 }}
    >
      {drag.event?.title ?? "(Без названия)"}
      <div className="font-normal opacity-85">
        {hhmm(drag.previewStart)}–{hhmm(drag.previewEnd)}
      </div>
    </div>
  );
}
