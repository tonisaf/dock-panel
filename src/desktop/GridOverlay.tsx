import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import { useAppearance } from "../lib/appearance";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** `GridView` in desktop.rs: physical px from the top-left of the work area. */
interface GridView {
  area: Rect;
  columns: number[];
  width: number;
  step: number;
  target: Rect | null;
}

/** Row lines every this many grid steps; every step would be a blur of lines. */
const ROW_EVERY = 4;

/**
 * The desktop grid, shown in a see-through window under a widget while it is
 * dragged: the columns, faint row lines and the spot the widget will land on.
 */
export function GridOverlay() {
  useAppearance();
  const [view, setView] = useState<GridView | null>(null);
  const [shown, setShown] = useState(false);
  // A new drag starts the target where it is instead of sliding it from the last drop.
  const [drag, setDrag] = useState(0);

  useEffect(() => {
    const unlisten = [
      listen<GridView>("grid:show", ({ payload }) => {
        setView(payload);
        setDrag((n) => n + 1);
        setShown(true);
      }),
      listen<GridView>("grid:update", ({ payload }) => setView(payload)),
      listen("grid:hide", () => setShown(false)),
    ];
    return () => unlisten.forEach((p) => p.then((fn) => fn()));
  }, []);

  if (!view) return null;
  const px = (v: number) => v / window.devicePixelRatio;
  const line = "color-mix(in srgb, var(--color-ink) 9%, transparent)";
  const { area, target } = view;
  return (
    <motion.div
      className="pointer-events-none fixed inset-0"
      initial={{ opacity: 0 }}
      animate={{ opacity: shown ? 1 : 0 }}
      transition={{ duration: shown ? 0.12 : 0.16, ease: "easeOut" }}
    >
      {view.columns.map((x) => (
        <div
          key={x}
          className="absolute rounded-2xl border border-dashed border-ink/20 bg-ink/[0.04]"
          style={{
            left: px(x),
            top: px(area.y),
            width: px(view.width),
            height: px(area.h),
            backgroundImage: `linear-gradient(to bottom, ${line} 1px, transparent 1px)`,
            backgroundSize: `100% ${px(view.step * ROW_EVERY)}px`,
          }}
        />
      ))}
      {target && (
        <motion.div
          key={drag}
          className="absolute top-0 left-0 rounded-2xl border-2 border-accent bg-accent/20 shadow-lg shadow-accent/20"
          initial={false}
          animate={{ x: px(target.x), y: px(target.y), width: px(target.w), height: px(target.h) }}
          transition={{ type: "spring", stiffness: 700, damping: 48 }}
        />
      )}
    </motion.div>
  );
}
