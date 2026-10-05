import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { MoreHorizontal, type LucideIcon } from "lucide-react";

export interface WidgetMenuItem {
  label: string;
  icon?: LucideIcon;
  disabled?: boolean;
  onClick: () => void;
}

export function WidgetMenu({ items }: { items: WidgetMenuItem[] }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  useEffect(() => {
    if (!position) return;
    popup.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const close = (e: PointerEvent) => {
      if (!popup.current?.contains(e.target as Node) && !trigger.current?.contains(e.target as Node)) setPosition(null);
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setPosition(null); trigger.current?.focus(); }
    };
    const dismiss = () => setPosition(null);
    document.addEventListener("pointerdown", close);
    window.addEventListener("keydown", escape, true);
    window.addEventListener("resize", dismiss);
    document.addEventListener("scroll", dismiss, true);
    return () => {
      document.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", escape, true);
      window.removeEventListener("resize", dismiss);
      document.removeEventListener("scroll", dismiss, true);
    };
  }, [position]);
  return (
    <>
      <button ref={trigger} type="button" className="widget-icon-button" aria-label="Действия виджета" aria-expanded={!!position} onClick={(e) => {
        e.stopPropagation();
        if (position) { setPosition(null); return; }
        const rect = e.currentTarget.getBoundingClientRect();
        setPosition({ top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - items.length * 40 - 16)), left: Math.max(8, Math.min(rect.right - 240, window.innerWidth - 248)) });
      }}><MoreHorizontal className="size-4" /></button>
      {position && createPortal(
        <div ref={popup} style={position} className="fixed z-[100] flex w-60 max-w-[calc(100vw-1rem)] flex-col gap-1 rounded-xl border border-stroke bg-popover p-1.5 shadow-lg" onClick={(e) => e.stopPropagation()} onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node)) setPosition(null);
        }}>
          {items.map(({ label, icon: Icon, disabled, onClick }) => (
            <button key={label} type="button" disabled={disabled} className="flex min-h-9 items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] font-normal text-fg-muted hover:bg-ink/10 hover:text-fg focus-visible:bg-ink/10 focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50" onClick={() => {
              setPosition(null); trigger.current?.focus(); onClick();
            }}>{Icon && <Icon className="size-4 shrink-0" />}{label}</button>
          ))}
        </div>, document.body,
      )}
    </>
  );
}
