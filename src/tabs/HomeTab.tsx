import { useEffect, useMemo, useState } from "react";
import { Reorder, useDragControls } from "motion/react";
import { Eye, EyeOff, GripVertical, Pin, SlidersHorizontal } from "lucide-react";
import clsx from "clsx";
import { Card } from "../components/Card";
import { AppGrid } from "../components/AppTile";
import { useAppsById, type AppEntry } from "../lib/apps";
import { usePrefs } from "../lib/prefs";
import { WIDGETS, type WidgetDef } from "../widgets/registry";

/** Pinned apps can be hidden too; it always stays on top. */
const PINNED_ID = "pinned";

/** Saved order first (skipping removed widgets), then any widgets added since. */
function useWidgetLayout() {
  const { widgetOrder, hiddenWidgets, setWidgetLayout } = usePrefs();
  const ordered = useMemo(() => {
    const byId = new Map(WIDGETS.map((w) => [w.id, w]));
    const known = widgetOrder.map((id) => byId.get(id)).filter((w): w is WidgetDef => !!w);
    return [...known, ...WIDGETS.filter((w) => !widgetOrder.includes(w.id))];
  }, [widgetOrder]);
  return { ordered, hidden: hiddenWidgets, save: setWidgetLayout };
}

function EyeButton({ visible, onClick }: { visible: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title={visible ? "Скрыть" : "Показать"}
      className="grid size-7 place-items-center rounded-lg text-fg-muted hover:bg-ink/10 hover:text-fg"
    >
      {visible ? <Eye className="size-4" /> : <EyeOff className="size-4" />}
    </button>
  );
}

function EditorItem({ widget, visible, onToggle }: { widget: WidgetDef; visible: boolean; onToggle: () => void }) {
  const controls = useDragControls();
  return (
    <Reorder.Item
      value={widget}
      dragListener={false}
      dragControls={controls}
      className="flex items-center gap-2 rounded-xl border border-stroke bg-surface px-2 py-1.5"
      whileDrag={{ scale: 1.02, boxShadow: "0 8px 24px rgb(0 0 0 / 0.25)" }}
    >
      <button
        onPointerDown={(e) => controls.start(e)}
        className="grid size-7 cursor-grab touch-none place-items-center rounded-lg text-fg-subtle hover:text-fg active:cursor-grabbing"
        title="Перетащите"
      >
        <GripVertical className="size-4" />
      </button>
      <span className={clsx("flex-1 text-[13.5px]", !visible && "text-fg-subtle line-through")}>{widget.title}</span>
      <EyeButton visible={visible} onClick={onToggle} />
    </Reorder.Item>
  );
}

/** Reorder by dragging, hide with the eye. Saves on "Готово". */
function LayoutEditor({ onDone }: { onDone: () => void }) {
  const { ordered, hidden, save } = useWidgetLayout();
  const [items, setItems] = useState(ordered);
  const [hiddenSet, setHiddenSet] = useState(() => new Set(hidden));
  const toggle = (id: string) =>
    setHiddenSet((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between px-1">
        <span className="text-[12px] text-fg-subtle">Перетащите за ⋮⋮, глаз — скрыть</span>
        <button
          onClick={() => {
            save(
              items.map((w) => w.id),
              [...hiddenSet],
            );
            onDone();
          }}
          className="rounded-lg bg-accent px-3 py-1 text-[12.5px] font-medium text-on-accent hover:bg-accent/90"
        >
          Готово
        </button>
      </div>
      <div className="flex items-center gap-2 rounded-xl border border-stroke bg-surface px-2 py-1.5">
        <span className="grid size-7 place-items-center text-fg-subtle">
          <Pin className="size-4" />
        </span>
        <span className={clsx("flex-1 text-[13.5px]", hiddenSet.has(PINNED_ID) && "text-fg-subtle line-through")}>
          Закреплённые приложения
        </span>
        <EyeButton visible={!hiddenSet.has(PINNED_ID)} onClick={() => toggle(PINNED_ID)} />
      </div>
      <Reorder.Group axis="y" values={items} onReorder={setItems} className="flex flex-col gap-2">
        {items.map((w) => (
          <EditorItem key={w.id} widget={w} visible={!hiddenSet.has(w.id)} onToggle={() => toggle(w.id)} />
        ))}
      </Reorder.Group>
    </div>
  );
}

const HOME_PINNED_MAX = 8;

function PinnedApps() {
  const byId = useAppsById();
  const pinnedIds = usePrefs((s) => s.pinned);
  const pinned = useMemo(
    () =>
      pinnedIds
        .map((id) => byId.get(id))
        .filter((a): a is AppEntry => !!a)
        .slice(0, HOME_PINNED_MAX),
    [pinnedIds, byId],
  );

  return (
    <Card title="Закреплённые" icon={Pin} className="hover:bg-surface">
      {pinned.length > 0 ? (
        <AppGrid apps={pinned} />
      ) : (
        <p className="text-[12px] leading-relaxed text-fg-subtle">
          Кликните правой кнопкой по приложению во вкладке «Приложения» и выберите «Закрепить».
        </p>
      )}
    </Card>
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
  const [editing, setEditing] = useState(false);
  const { ordered, hidden } = useWidgetLayout();
  const time = now.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  const date = now.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" });

  return (
    <div className="flex flex-col gap-3 pb-2">
      <div className="flex items-end justify-between px-1 pt-1 pb-2">
        <div>
          <div className="font-display text-[52px] leading-none font-semibold tracking-tight tabular-nums">{time}</div>
          <div className="mt-1.5 text-[14px] text-fg-muted first-letter:uppercase">{date}</div>
        </div>
        {!editing && (
          <button
            onClick={() => setEditing(true)}
            title="Настроить главную"
            className="grid size-8 place-items-center rounded-lg text-fg-subtle hover:bg-ink/8 hover:text-fg"
          >
            <SlidersHorizontal className="size-4" />
          </button>
        )}
      </div>

      {editing ? (
        <LayoutEditor onDone={() => setEditing(false)} />
      ) : (
        <>
          {!hidden.includes(PINNED_ID) && <PinnedApps />}

          {/* Masonry: a wider panel gets more widget columns instead of wider widgets. */}
          <div className="columns-[340px] gap-3">
            {ordered
              .filter((w) => !hidden.includes(w.id))
              .map(({ id, component: Widget }) => (
                <div key={id} className="mb-3 break-inside-avoid">
                  <Widget />
                </div>
              ))}
          </div>
        </>
      )}
    </div>
  );
}
