import clsx from "clsx";
import { effectiveWindows, formatIn, levelClass, windowLabel, type Snapshot } from "./api";

/** One row per rate-limit window: label, bar, percent, time to reset. */
export function LimitBars({ snapshot, large = false }: { snapshot: Snapshot; large?: boolean }) {
  return (
    <div className={clsx("flex flex-col", large ? "gap-3" : "gap-2")}>
      {effectiveWindows(snapshot).map((w) => (
        <div key={w.kind} className="flex items-center gap-2.5 text-[12px]">
          <span className="w-14 shrink-0 text-fg-muted">{windowLabel(w.kind)}</span>
          <div className={clsx("flex-1 overflow-hidden rounded-full bg-ink/10", large ? "h-2" : "h-1.5")}>
            <div
              className={clsx("h-full rounded-full transition-[width] duration-700", levelClass(w.usedPercent))}
              style={{ width: `${Math.min(100, w.usedPercent)}%` }}
            />
          </div>
          <span className="w-9 shrink-0 text-right font-medium tabular-nums">{Math.round(w.usedPercent)}%</span>
          <span className="w-20 shrink-0 text-right text-fg-subtle tabular-nums">
            {w.reset ? "сброшен" : formatIn(w.resetsAt)}
          </span>
        </div>
      ))}
    </div>
  );
}
