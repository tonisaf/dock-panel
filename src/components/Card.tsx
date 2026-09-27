import type { ReactNode } from "react";
import { ChevronDown, type LucideIcon } from "lucide-react";
import clsx from "clsx";
import { Collapse } from "./Collapse";

export function Card({
  title,
  icon: Icon,
  className,
  action,
  summary,
  collapsed,
  onToggle,
  children,
}: {
  title?: string;
  icon?: LucideIcon;
  className?: string;
  /** Small controls at the right end of the header. */
  action?: ReactNode;
  /** Short text shown in the header while collapsed. */
  summary?: ReactNode;
  /** With `onToggle`, the header folds the card to just itself. */
  collapsed?: boolean;
  onToggle?: () => void;
  children: ReactNode;
}) {
  const collapsible = onToggle != null;
  return (
    <section
      className={clsx(
        "rounded-2xl border border-stroke bg-surface p-3.5 transition-colors hover:bg-surface-hover",
        className,
      )}
    >
      {title && (
        <header
          role={collapsible ? "button" : undefined}
          aria-expanded={collapsible ? !collapsed : undefined}
          onClick={onToggle}
          className={clsx(
            "flex items-center gap-1.5 text-[12px] font-medium text-fg-muted",
            // A collapsible header gets a padded hit area without moving its text.
            !collapsible && "mb-2",
            collapsible && "-mx-1 -mt-1 cursor-default rounded-lg p-1 hover:text-fg",
            collapsible && (collapsed ? "-mb-1" : "mb-1"),
          )}
        >
          {Icon && <Icon className="size-3.5" strokeWidth={2.2} />}
          {title}
          {collapsible && collapsed && summary && (
            <span className="truncate font-normal text-fg-subtle">· {summary}</span>
          )}
          {(action || collapsible) && (
            <div className="-my-1 ml-auto flex items-center gap-0.5" onClick={(e) => e.stopPropagation()}>
              {action}
              {collapsible && (
                <button
                  onClick={onToggle}
                  title={collapsed ? "Развернуть" : "Свернуть"}
                  className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
                >
                  <ChevronDown className={clsx("size-3.5 transition-transform", !collapsed && "rotate-180")} />
                </button>
              )}
            </div>
          )}
        </header>
      )}
      {collapsible ? (
        <Collapse open={!collapsed} className="-mx-2 px-2">
          {children}
        </Collapse>
      ) : (
        children
      )}
    </section>
  );
}

/** Placeholder body for things that arrive in a later phase. */
export function Soon({ phase, note }: { phase: number; note: string }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="h-2.5 w-3/4 rounded-full bg-ink/8" />
      <div className="h-2.5 w-1/2 rounded-full bg-ink/6" />
      <p className="mt-1 text-[11px] text-fg-subtle">
        Этап {phase} · {note}
      </p>
    </div>
  );
}

export function EmptyState({ icon: Icon, title, text }: { icon: LucideIcon; title: string; text: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-8 pb-16 text-center">
      <div className="mb-1 grid size-12 place-items-center rounded-2xl border border-stroke bg-surface">
        <Icon className="size-5 text-accent" />
      </div>
      <h2 className="text-[15px] font-semibold">{title}</h2>
      <p className="text-[13px] leading-relaxed text-fg-muted">{text}</p>
    </div>
  );
}
