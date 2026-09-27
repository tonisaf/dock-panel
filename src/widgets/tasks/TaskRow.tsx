import { useState } from "react";
import { motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, CalendarDays } from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../../store";
import { describeDue, notionColor, type Badge, type Task } from "./api";
import { TaskEditor } from "./TaskEditor";

function Chip({ badge }: { badge: Badge }) {
  const color = notionColor(badge.color);
  return (
    <span
      className="rounded-[5px] px-1.5 py-px text-[10.5px] leading-4 font-medium"
      style={{ color, backgroundColor: `color-mix(in srgb, ${color} 16%, transparent)` }}
    >
      {badge.name}
    </span>
  );
}

export function TaskRow({
  task,
  canComplete,
  onComplete,
  compact = false,
}: {
  task: Task;
  canComplete: boolean;
  onComplete: (task: Task) => void;
  compact?: boolean;
}) {
  const [checked, setChecked] = useState(false);
  // In the tasks tab a click opens the editor; the compact widget row opens Notion.
  const [editing, setEditing] = useState(false);
  const due = task.due ? describeDue(task.due) : null;

  const complete = () => {
    if (checked) return;
    setChecked(true);
    // Let the check animation play before the row leaves.
    setTimeout(() => onComplete(task), 280);
  };

  const open = () => {
    if (!task.url) return;
    openUrl(task.url).catch(console.error);
    usePanelStore.getState().setOpen(false);
  };

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: -16, transition: { duration: 0.18 } }}
      className="group flex items-start gap-2.5 rounded-xl px-2 py-1.5 hover:bg-surface"
    >
      <button
        onClick={complete}
        disabled={!canComplete}
        title="Отметить выполненной"
        className={clsx(
          "mt-0.5 grid size-[18px] shrink-0 place-items-center rounded-full border transition-colors",
          checked
            ? "border-emerald-400 bg-emerald-400 text-black"
            : task.inProgress
              ? "border-accent/70 hover:bg-accent/20"
              : "border-ink/30 hover:border-ink/60 hover:bg-ink/10",
        )}
      >
        {checked && <Check className="size-3" strokeWidth={3} />}
      </button>

      <div className="min-w-0 flex-1">
        <button
          onClick={compact ? open : () => setEditing(!editing)}
          title={compact ? "Открыть в Notion" : "Изменить"}
          className={clsx(
            "block w-full text-left leading-snug hover:underline",
            compact ? "truncate text-[13px]" : "text-[13.5px]",
            checked && "text-fg-subtle line-through",
          )}
        >
          {task.title || "Без названия"}
        </button>
        {(due || task.priority || task.tag || task.inProgress) && (
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            {task.priority && <Chip badge={task.priority} />}
            {!compact && task.tag && <Chip badge={task.tag} />}
            {task.inProgress && task.status && <Chip badge={task.status} />}
            {due && (
              <span
                className={clsx(
                  "flex items-center gap-1 text-[11px]",
                  due.overdue ? "text-danger" : due.soon ? "text-warn" : "text-fg-subtle",
                )}
              >
                <CalendarDays className="size-3" /> {due.label}
              </span>
            )}
          </div>
        )}
        {editing && <TaskEditor task={task} onClose={() => setEditing(false)} />}
      </div>
    </motion.div>
  );
}
