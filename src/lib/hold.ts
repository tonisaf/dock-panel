import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";

/** How long to hold the mouse on a widget before it lifts off for moving. */
const HOLD_MS = 400;
/** Moving further than this before then is a scroll or a text selection, not a hold. */
const HOLD_SLOP = 6;

/** Controls where holding the mouse means using them, not moving the widget. */
function isField(target: EventTarget | null) {
  const el = target instanceof Element ? target : null;
  return !!el?.closest("input, textarea, select, [contenteditable=''], [contenteditable='true'], [data-no-hold]");
}

/**
 * Press and hold on a widget to lift it: calls `onLift` with the element and
 * the press point once the mouse has stayed down (and nearly still) for HOLD_MS.
 */
export function useHoldToLift<T>(onLift: (key: T, el: HTMLElement, start: { x: number; y: number }) => void) {
  const cancel = useRef<(() => void) | null>(null);
  useEffect(() => () => cancel.current?.(), []);
  return (e: ReactPointerEvent<HTMLElement>, key: T) => {
    if (e.button !== 0 || isField(e.target)) return;
    cancel.current?.();
    const el = e.currentTarget;
    const start = { x: e.clientX, y: e.clientY };
    const onMove = (m: PointerEvent) => {
      if (Math.hypot(m.clientX - start.x, m.clientY - start.y) > HOLD_SLOP) stop();
    };
    const timer = setTimeout(() => {
      stop();
      window.getSelection()?.removeAllRanges();
      onLift(key, el, start);
    }, HOLD_MS);
    const stop = () => {
      clearTimeout(timer);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      cancel.current = null;
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    cancel.current = stop;
  };
}
