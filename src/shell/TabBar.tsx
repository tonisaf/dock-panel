import { motion } from "motion/react";
import clsx from "clsx";
import { TABS, usePanelStore } from "../store";

export function TabBar() {
  const { tab, setTab } = usePanelStore();

  return (
    <nav className="flex shrink-0 gap-1 rounded-xl border border-stroke bg-surface p-1">
      {TABS.map(({ id, label, icon: Icon }, i) => {
        const active = id === tab;
        return (
          <button
            key={id}
            onClick={() => setTab(id)}
            title={`${label}  (Ctrl+${i + 1})`}
            className={clsx(
              "relative flex flex-1 flex-col items-center gap-0.5 rounded-lg py-1.5 text-[11px] font-medium outline-none transition-colors",
              active ? "text-fg" : "text-fg-muted hover:text-fg",
            )}
          >
            {active && (
              <motion.span
                layoutId="tab-pill"
                className="absolute inset-0 rounded-lg bg-ink/10 shadow-[inset_0_0_0_1px_var(--color-stroke)]"
                transition={{ type: "spring", stiffness: 500, damping: 38 }}
              />
            )}
            <Icon className={clsx("relative size-[18px]", active && "text-accent")} strokeWidth={2} />
            <span className="relative">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}
