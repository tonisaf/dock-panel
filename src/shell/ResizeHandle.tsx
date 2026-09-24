import { useRef, type PointerEvent } from "react";
import clsx from "clsx";
import { usePanelWidth } from "../lib/panelWidth";

/** Drag the panel's inner edge (the one facing the screen) to resize it. */
export function ResizeHandle() {
  const { width, edge, setWidth } = usePanelWidth();
  const sign = edge === "right" ? -1 : 1;
  const drag = useRef<{ startX: number; startWidth: number; frame: number; latest: number } | null>(null);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { startX: e.screenX, startWidth: width, frame: 0, latest: width };
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    d.latest = d.startWidth + sign * (e.screenX - d.startX);
    // One resize per frame; the window resize itself is the expensive part.
    if (!d.frame) {
      d.frame = requestAnimationFrame(() => {
        d.frame = 0;
        setWidth(d.latest, false);
      });
    }
  };

  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    cancelAnimationFrame(d.frame);
    drag.current = null;
    e.currentTarget.releasePointerCapture(e.pointerId);
    setWidth(d.latest, true);
  };

  return (
    <div
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      title="Потяните, чтобы изменить ширину"
      className={clsx("group absolute inset-y-0 z-40 w-2 cursor-ew-resize", edge === "right" ? "left-0" : "right-0")}
    >
      <div
        className={clsx(
          "absolute inset-y-6 w-0.5 rounded-full bg-accent/0 transition-colors group-hover:bg-accent/60 group-active:bg-accent",
          edge === "right" ? "left-0.5" : "right-0.5",
        )}
      />
    </div>
  );
}
