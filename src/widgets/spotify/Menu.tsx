import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { motion } from "motion/react";

/**
 * A small popup menu under (or above) `anchor`, kept inside the window.
 * Clicks outside, the wheel and Esc close it.
 */
export function AnchoredMenu({
  anchor,
  onClose,
  className,
  children,
}: {
  anchor: RefObject<HTMLElement | null>;
  onClose: () => void;
  className?: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: -9999, y: -9999 });

  useLayoutEffect(() => {
    if (!anchor.current || !ref.current) return;
    const b = anchor.current.getBoundingClientRect();
    const { width, height } = ref.current.getBoundingClientRect();
    const x = Math.max(8, Math.min(b.right - width, window.innerWidth - width - 8));
    const y = b.bottom + height + 12 > window.innerHeight ? Math.max(8, b.top - height - 4) : b.bottom + 4;
    // Runs after every render (the content changes size as it loads); only moves when it must.
    setPos((p) => (p.x === x && p.y === y ? p : { x, y }));
  });

  useEffect(() => {
    const close = (e: Event) => {
      const t = e.target as Node;
      if (!ref.current?.contains(t) && !anchor.current?.contains(t)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("wheel", close);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("wheel", close);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [anchor, onClose]);

  return createPortal(
    <motion.div
      ref={ref}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.1 }}
      style={{ left: pos.x, top: pos.y }}
      className={
        "fixed z-50 rounded-xl border border-ink/10 bg-popover p-1 text-fg shadow-2xl shadow-black/50 " + (className ?? "")
      }
    >
      {children}
    </motion.div>,
    document.body,
  );
}

export const menuItem = "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-ink/10";
