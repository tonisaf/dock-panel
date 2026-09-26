import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Circle, CircleCheck } from "lucide-react";
import { useGcalActions, ymd, type GTask } from "./api";

/** Each day's unfinished tasks; overdue ones gather on today, as in Google Calendar. */
export function tasksForDays(days: Date[], tasks: GTask[], now: Date) {
  const todayKey = ymd(now);
  return days.map((d) => {
    const key = ymd(d);
    return tasks.filter((t) => t.due && (t.due === key || (key === todayKey && t.due < todayKey)));
  });
}

/** A day's tasks as one chip; click lists them, a click on one completes it. */
export function TasksChip({ tasks }: { tasks: GTask[] }) {
  const { completeTask } = useGcalActions();
  const [open, setOpen] = useState<{ x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  if (tasks.length === 0) return null;
  const label = tasks.length === 1 ? tasks[0].title : `${tasks.length} ${plural(tasks.length)}`;
  return (
    <>
      <button
        onClick={(e) => setOpen({ x: e.clientX, y: e.clientY })}
        className="flex w-full items-center gap-1 truncate rounded-md bg-accent/15 px-1.5 py-0.5 text-left text-[11.5px] font-medium text-fg hover:bg-accent/25"
      >
        <CircleCheck className="size-3.5 shrink-0 text-accent" />
        <span className="truncate">{label}</span>
      </button>
      {open &&
        createPortal(
          <div
            ref={ref}
            style={{ left: Math.min(open.x, window.innerWidth - 328), top: open.y + 8 }}
            className="fixed z-50 flex max-h-80 w-80 flex-col gap-0.5 overflow-y-auto rounded-xl border border-ink/10 bg-popover p-1.5 text-fg shadow-2xl shadow-black/40"
          >
            {tasks.map((t) => (
              <button
                key={t.id}
                onClick={() => completeTask(t).catch(console.error)}
                title="Отметить выполненной"
                className="group flex items-start gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-ink/8"
              >
                <Circle className="mt-0.5 size-4 shrink-0 text-fg-subtle group-hover:hidden" />
                <CircleCheck className="mt-0.5 hidden size-4 shrink-0 text-accent group-hover:block" />
                <span className="min-w-0">
                  <span className="block text-[13px]">{t.title}</span>
                  <span className="block text-[11px] text-fg-subtle">
                    {t.list}
                    {t.due && ` · ${new Date(`${t.due}T00:00`).toLocaleDateString("ru-RU", { day: "numeric", month: "short" })}`}
                  </span>
                </span>
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

function plural(n: number) {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return "невыполненная задача";
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return "невыполненные задачи";
  return "невыполненных задач";
}
