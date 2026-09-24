import { AnimatePresence, motion } from "motion/react";
import { CheckCircle2 } from "lucide-react";
import type { Task } from "./api";

export function UndoBar({ task, onUndo }: { task: Task | null; onUndo: () => void }) {
  return (
    <AnimatePresence>
      {task && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 8 }}
          className="flex items-center gap-2 rounded-xl border border-ink/10 bg-popover px-3 py-2 text-[12.5px] shadow-lg shadow-black/40"
        >
          <CheckCircle2 className="size-4 shrink-0 text-ok" />
          <span className="min-w-0 flex-1 truncate text-fg-muted">«{task.title}» выполнена</span>
          <button onClick={onUndo} className="shrink-0 font-medium text-accent hover:underline">
            Отменить
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
