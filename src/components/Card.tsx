import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import clsx from "clsx";

export function Card({
  title,
  icon: Icon,
  className,
  children,
}: {
  title?: string;
  icon?: LucideIcon;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section
      className={clsx(
        "rounded-2xl border border-stroke bg-surface p-3.5 transition-colors hover:bg-surface-hover",
        className,
      )}
    >
      {title && (
        <header className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-fg-muted">
          {Icon && <Icon className="size-3.5" strokeWidth={2.2} />}
          {title}
        </header>
      )}
      {children}
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
