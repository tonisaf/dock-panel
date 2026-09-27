import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "motion/react";

/** The reading pane's width when the window grows for it. */
const READER_W = 560;
/** A panel at least this wide has room for the pane without growing. */
const WIDE_PANEL = 1050;
/** Below this much extra room the pane replaces the list instead. */
const READER_MIN = 360;

/** The list's narrowest width next to an open pane. */
const LIST_MIN = 280;
const DIVIDER = 12;

const setExtraWidth = (extra: number, animate = false) => invoke<number>("panel_set_extra_width", { extra, animate });

export type PaneMode = "split" | "full";

/**
 * An item open in a reading pane and where it shows: next to the list
 * ("split", growing the window with an animation when the panel is narrow)
 * or, when the screen has no room, in place of the list ("full"). Closing
 * shrinks the window back first, keeping the split layout until it has, so
 * the list doesn't jump.
 */
export function useSidePane<T>(baseWidth: number) {
  const [item, setItem] = useState<T | null>(null);
  const [mode, setMode] = useState<PaneMode | null>(null);
  const [closing, setClosing] = useState(false);
  // Refs mirror the state for the async steps; `turn` cancels outdated ones.
  const state = useRef({ mode: null as PaneMode | null, closing: false, grown: false, turn: 0 });
  const update = (patch: Partial<typeof state.current>) => {
    Object.assign(state.current, patch);
    if ("mode" in patch) setMode(patch.mode ?? null);
    if ("closing" in patch) setClosing(!!patch.closing);
  };

  const open = async (next: T) => {
    setItem(next);
    const st = state.current;
    if (st.mode && !st.closing) return; // already open: just another item
    const turn = ++st.turn;
    update({ mode: "split", closing: false });
    if (baseWidth >= WIDE_PANEL) return;
    const applied = await setExtraWidth(READER_W, true).catch(() => 0);
    if (turn !== state.current.turn) return;
    state.current.grown = applied > 0;
    if (applied < READER_MIN) {
      // No room on screen: the pane takes the list's place instead.
      state.current.grown = false;
      setExtraWidth(0).catch(console.error);
      update({ mode: "full" });
    }
  };

  const close = async () => {
    const st = state.current;
    const turn = ++st.turn;
    if (st.mode === "split" && st.grown) {
      update({ closing: true });
      await setExtraWidth(0, true).catch(console.error);
      if (turn !== state.current.turn) return; // reopened meanwhile
      state.current.grown = false;
    }
    setItem(null);
    update({ mode: null, closing: false });
  };

  // Leaving the tab gives the width back at once.
  useEffect(() => () => void setExtraWidth(0).catch(console.error), []);

  return { item, mode, closing, open, close };
}

/** Drag handle between the list and the pane; moves the split, not the window. */
function Divider({ width, max, onChange }: { width: number; max: number; onChange: (w: number, done: boolean) => void }) {
  const drag = useRef<{ x: number; start: number } | null>(null);
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(LIST_MIN, w)));
  return (
    <div
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, start: width };
      }}
      onPointerMove={(e) => drag.current && onChange(clamp(drag.current.start + e.clientX - drag.current.x), false)}
      onPointerUp={(e) => {
        if (!drag.current) return;
        onChange(clamp(drag.current.start + e.clientX - drag.current.x), true);
        drag.current = null;
      }}
      onDoubleClick={() => onChange(-1, true)}
      title="Потяните, чтобы изменить ширину. Двойной клик — как было"
      className="group relative shrink-0 cursor-col-resize touch-none"
      style={{ width: DIVIDER }}
    >
      <div className="absolute inset-y-2 left-1/2 w-px -translate-x-1/2 bg-stroke transition-colors group-hover:w-0.5 group-hover:bg-accent/60 group-active:bg-accent" />
    </div>
  );
}

/** The list and the open pane side by side, with a draggable split remembered in `savedWidth`. */
export function SplitView({
  baseWidth,
  closing,
  savedWidth,
  onSaveWidth,
  list,
  pane,
}: {
  baseWidth: number;
  closing: boolean;
  savedWidth: number | null;
  onSaveWidth: (w: number) => void;
  list: ReactNode;
  pane: ReactNode;
}) {
  const [draft, setDraft] = useState<number | null>(null);
  // Sizes against the window as it will be once grown, not as it is mid-animation.
  const content = (baseWidth >= WIDE_PANEL ? baseWidth : baseWidth + READER_W) - 32;
  const maxList = Math.max(LIST_MIN, content - READER_MIN - DIVIDER);
  const defaultList = baseWidth >= WIDE_PANEL ? Math.min(440, Math.round(baseWidth * 0.42)) : baseWidth - 32;
  const listWidth = Math.min(maxList, Math.max(LIST_MIN, draft ?? savedWidth ?? defaultList));
  const resize = (w: number, done: boolean) => {
    if (w < 0) {
      // Double click: back to the default split.
      setDraft(null);
      onSaveWidth(defaultList);
      return;
    }
    setDraft(done ? null : w);
    if (done) onSaveWidth(w);
  };
  return (
    <div className="flex h-full">
      <div className="scroll-area -mr-1 shrink-0 pr-1" style={{ width: listWidth }}>
        {list}
      </div>
      <Divider width={listWidth} max={maxList} onChange={resize} />
      <motion.div
        className="scroll-area min-w-0 flex-1 pr-1"
        initial={{ opacity: 0, x: 16 }}
        animate={closing ? { opacity: 0, x: 16 } : { opacity: 1, x: 0 }}
        transition={{ duration: 0.2, ease: "easeOut" }}
      >
        {pane}
      </motion.div>
    </div>
  );
}
