import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import clsx from "clsx";

/**
 * Content that folds away by animating its height. It clips while it moves,
 * so content with negative side margins (hover rows) needs `className` to pad
 * the clip box out, e.g. "-mx-2 px-2".
 */
export function Collapse({ open, className, children }: { open: boolean; className?: string; children: ReactNode }) {
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          key="content"
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: "auto", opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.2, ease: [0.2, 0, 0, 1] }}
          className={clsx("overflow-hidden", className)}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
