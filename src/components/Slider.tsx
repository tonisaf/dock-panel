import { useEffect, useRef, useState } from "react";

/**
 * Range input that shows the value while dragging and sends it once on
 * release: devices (lamps, monitors) accept only a few commands per second.
 */
export function Slider({
  value,
  min,
  max,
  step = 1,
  onCommit,
  onDraft,
  track,
  label,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onCommit: (v: number) => void;
  /** The value while dragging, before it is committed. */
  onDraft?: (v: number | null) => void;
  track?: string;
  label: string;
}) {
  const [draft, setDraftState] = useState<number | null>(null);
  const setDraft = (v: number | null) => {
    setDraftState(v);
    onDraft?.(v);
  };
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const commit = () => {
    clearTimeout(timer.current);
    if (draft != null && draft !== value) onCommit(draft);
    setDraft(null);
  };
  // Arrow-key presses in a row become one command.
  const commitSoon = () => {
    clearTimeout(timer.current);
    timer.current = setTimeout(commit, 400);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <input
      type="range"
      aria-label={label}
      min={min}
      max={max}
      step={step}
      value={draft ?? value}
      onChange={(e) => setDraft(Number(e.target.value))}
      onPointerUp={commit}
      onKeyUp={commitSoon}
      onBlur={commit}
      className="h-1.5 w-full cursor-pointer appearance-none rounded-full accent-accent"
      style={{ background: track ?? "color-mix(in srgb, var(--color-ink) 12%, transparent)" }}
    />
  );
}
