import { motion } from "motion/react";
import clsx from "clsx";
import { TABS, usePanelStore, type TabId } from "../store";
import { useUnread } from "../mail/api";

/**
 * The tab switcher. In full screen it drives the right side only: home is
 * always on the left, so it's left out (`exclude`) and `active` is that side's tab.
 */
export function TabBar({ exclude, active: activeTab }: { exclude?: TabId; active?: TabId } = {}) {
  const { tab, setTab } = usePanelStore();
  const unread = useUnread().data?.total ?? 0;
  const current = activeTab ?? tab;

  return (
    <nav className="@container flex shrink-0 gap-1 rounded-xl border border-stroke bg-surface p-1">
      {TABS.map(({ id, label, icon: Icon }, i) => {
        if (id === exclude) return null;
        const active = id === current;
        return (
          <button
            key={id}
            onClick={() => setTab(id)}
            title={`${label}  (Ctrl+${i + 1})`}
            className={clsx(
              "relative flex min-w-0 flex-auto flex-col items-center gap-0.5 rounded-lg px-1 py-1.5 text-[11px] font-medium outline-none transition-colors",
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
            {/* Eight labels need a tighter font on the narrowest panel; tabs size to their labels. */}
            <span className="relative max-w-full truncate @max-[480px]:text-[10px] @max-[480px]:tracking-tight">
              {label}
            </span>
          </button>
        );
      })}
    </nav>
  );
}
