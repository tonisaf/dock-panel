import { motion } from "motion/react";
import clsx from "clsx";
import { TABS, usePanelStore } from "../store";
import { useUnread } from "../mail/api";

export function TabBar() {
  const { tab, setTab } = usePanelStore();
  const unread = useUnread().data?.total ?? 0;

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
            <span className="relative">
              <Icon className={clsx("size-[18px]", active && "text-accent")} strokeWidth={2} />
              {id === "mail" && unread > 0 && (
                <span className="absolute -top-1.5 left-3 min-w-4 rounded-full bg-accent px-1 text-center text-[9.5px] leading-4 font-semibold text-on-accent tabular-nums">
                  {unread > 99 ? "99+" : unread}
                </span>
              )}
            </span>
            <span className="relative">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}
