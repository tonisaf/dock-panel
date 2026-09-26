import { motion } from "motion/react";
import clsx from "clsx";

export function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={clsx(
        "flex h-6 w-11 shrink-0 items-center rounded-full border px-0.5 transition-colors",
        on ? "justify-end border-accent bg-accent" : "justify-start border-ink/25 bg-transparent",
      )}
    >
      <motion.span layout className={clsx("size-4 rounded-full", on ? "bg-on-accent" : "bg-ink/70")} />
    </button>
  );
}
