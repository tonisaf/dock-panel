import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { motion } from "motion/react";
import clsx from "clsx";
import { launchApp, type AppEntry } from "../lib/apps";
import { FOLDER_PREFIX, usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { AnchoredMenu } from "../widgets/spotify/Menu";
import { AppIcon } from "./AppIcon";

/** How far the pointer moves before a press becomes a drag. */
const DRAG_SLOP = 5;

function FolderIcon({ items, size }: { items: AppEntry[]; size: number }) {
  const mini = Math.round(size * 0.36);
  return (
    <div
      className="grid grid-cols-2 place-content-center place-items-center gap-[3px] rounded-xl bg-ink/10 p-[5px]"
      style={{ width: size, height: size }}
    >
      {items.slice(0, 4).map((a) => (
        <AppIcon key={a.id} id={a.id} size={mini} />
      ))}
    </div>
  );
}

/** A folder's apps in a popup; its name is edited in place. */
function FolderPopup({ entry, anchor, onClose }: { entry: AppEntry; anchor: RefObject<HTMLElement | null>; onClose: () => void }) {
  const renameFolder = usePrefs((s) => s.renameFolder);
  const setMenu = usePanelStore((s) => s.setMenu);
  const [name, setName] = useState(entry.name);
  const key = entry.folder!.key;
  return (
    <AnchoredMenu anchor={anchor} onClose={onClose} className="w-72 p-2">
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        onBlur={() => renameFolder(key, name)}
        onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
        className="mb-1.5 h-8 w-full rounded-lg bg-transparent px-2 text-center text-[13px] font-medium outline-none hover:bg-ink/6 focus:bg-ink/8"
        title="Название папки"
      />
      <div className="grid grid-cols-3 gap-1">
        {entry.folder!.items.map((a) => (
          <button
            key={a.id}
            onClick={() => launchApp(a.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ appId: a.id, x: e.clientX, y: e.clientY });
            }}
            title={a.name}
            className="flex flex-col items-center gap-1.5 rounded-xl px-1 pt-2 pb-1.5 hover:bg-ink/8"
          >
            <AppIcon id={a.id} size={36} />
            <span className="line-clamp-2 w-full text-center text-[11px] leading-tight text-fg-muted">{a.name}</span>
          </button>
        ))}
      </div>
    </AnchoredMenu>
  );
}

function Tile({
  entry,
  dragging,
  mergeTarget,
  onPointerDown,
}: {
  entry: AppEntry;
  dragging: boolean;
  mergeTarget: boolean;
  onPointerDown: (e: ReactPointerEvent, id: string) => void;
}) {
  const setMenu = usePanelStore((s) => s.setMenu);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <motion.div layout="position" transition={{ type: "spring", stiffness: 520, damping: 40 }} data-pin-id={entry.id}>
      <button
        ref={ref}
        onPointerDown={(e) => onPointerDown(e, entry.id)}
        onClick={() => (entry.folder ? setOpen(!open) : launchApp(entry.id))}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ appId: entry.id, x: e.clientX, y: e.clientY });
        }}
        title={entry.folder ? `${entry.name}: ${entry.folder.items.map((a) => a.name).join(", ")}` : entry.name}
        className={clsx(
          "group flex w-full flex-col items-center gap-1.5 rounded-xl px-1 pt-2.5 pb-2 outline-none transition-[background-color,opacity] hover:bg-surface-hover focus-visible:bg-surface-hover",
          dragging && "opacity-30",
          mergeTarget && "bg-accent/15 ring-2 ring-accent/60",
        )}
      >
        <div className="transition-transform duration-150 group-hover:-translate-y-0.5">
          {entry.folder ? <FolderIcon items={entry.folder.items} size={40} /> : <AppIcon id={entry.id} size={40} />}
        </div>
        <span className="line-clamp-2 w-full text-center text-[11.5px] leading-tight text-fg-muted group-hover:text-fg">{entry.name}</span>
      </button>
      {open && entry.folder && <FolderPopup entry={entry} anchor={ref} onClose={close} />}
    </motion.div>
  );
}

interface Drag {
  id: string;
  pointerId: number;
  x0: number;
  y0: number;
  active: boolean;
}

/**
 * The pinned apps as a grid you rearrange by dragging. Dropping an app on the
 * middle of another makes a folder of the two (or adds it to that folder);
 * dropping between tiles moves it there.
 */
export function PinnedGrid({ entries }: { entries: AppEntry[] }) {
  const { setPinned, groupPinned } = usePrefs.getState();
  const [order, setOrder] = useState<string[] | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [merge, setMerge] = useState<string | null>(null);
  const [ghost, setGhost] = useState<{ x: number; y: number } | null>(null);
  const drag = useRef<Drag | null>(null);
  const liveOrder = useRef<string[]>([]);
  const liveMerge = useRef<string | null>(null);

  const byId = new Map(entries.map((e) => [e.id, e]));
  const ids = order ?? entries.map((e) => e.id);
  const shown = ids.map((id) => byId.get(id)).filter((e): e is AppEntry => !!e);

  const onPointerDown = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0) return;
    drag.current = { id, pointerId: e.pointerId, x0: e.clientX, y0: e.clientY, active: false };
  };

  useEffect(() => {
    const move = (e: PointerEvent) => {
      const d = drag.current;
      if (!d || e.pointerId !== d.pointerId) return;
      if (!d.active) {
        if (Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < DRAG_SLOP) return;
        d.active = true;
        liveOrder.current = entries.map((x) => x.id);
        setOrder(liveOrder.current);
        setDragId(d.id);
      }
      setGhost({ x: e.clientX, y: e.clientY });
      const over = document
        .elementsFromPoint(e.clientX, e.clientY)
        .map((el) => el.closest<HTMLElement>("[data-pin-id]"))
        .find((el) => el && el.dataset.pinId !== d.id);
      if (!over) return setMergeTarget(null);
      const target = over.dataset.pinId!;
      const r = over.getBoundingClientRect();
      const inMiddle = Math.abs(e.clientX - (r.left + r.width / 2)) < r.width * 0.22 && Math.abs(e.clientY - (r.top + r.height / 2)) < r.height * 0.3;
      // Folders don't go into folders.
      if (inMiddle && !d.id.startsWith(FOLDER_PREFIX)) return setMergeTarget(target);
      setMergeTarget(null);
      const list = liveOrder.current.filter((x) => x !== d.id);
      const at = list.indexOf(target) + (e.clientX > r.left + r.width / 2 ? 1 : 0);
      list.splice(at, 0, d.id);
      if (list.join() !== liveOrder.current.join()) {
        liveOrder.current = list;
        setOrder(list);
      }
    };
    const setMergeTarget = (t: string | null) => {
      liveMerge.current = t;
      setMerge(t);
    };
    const up = (e: PointerEvent) => {
      const d = drag.current;
      if (!d || e.pointerId !== d.pointerId) return;
      drag.current = null;
      if (!d.active) return;
      // The release ends a drag, not a click.
      const swallow = (ev: Event) => ev.stopPropagation();
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 50);
      if (liveMerge.current) groupPinned(d.id, liveMerge.current);
      else setPinned(mergeInto(usePrefs.getState().pinned, liveOrder.current));
      setOrder(null);
      setDragId(null);
      setMergeTarget(null);
      setGhost(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, [entries, groupPinned, setPinned]);

  const dragged = dragId ? byId.get(dragId) : null;
  return (
    <div data-no-hold className="grid touch-none grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-1">
      {shown.map((entry) => (
        <Tile key={entry.id} entry={entry} dragging={entry.id === dragId} mergeTarget={entry.id === merge} onPointerDown={onPointerDown} />
      ))}
      {dragged &&
        ghost &&
        createPortal(
          <div className="pointer-events-none fixed z-50 -translate-x-1/2 -translate-y-1/2 drop-shadow-xl" style={{ left: ghost.x, top: ghost.y }}>
            {dragged.folder ? <FolderIcon items={dragged.folder.items} size={44} /> : <AppIcon id={dragged.id} size={44} />}
          </div>,
          document.body,
        )}
    </div>
  );
}

/**
 * The full pinned list in the order the visible part was dragged into: the
 * home widget shows only the first few, and the rest keep their places after them.
 */
function mergeInto(all: string[], visible: string[]) {
  const shown = new Set(visible);
  return [...visible, ...all.filter((id) => !shown.has(id))];
}
