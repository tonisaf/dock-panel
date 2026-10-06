import { AnimatePresence } from "motion/react";
import { CheckSquare, ChevronRight } from "lucide-react";
import { WidgetState } from "../../components/WidgetState";
import { Card } from "../../components/Card";
import { useTasksSource } from "../../lib/integrations";
import { usePanelStore } from "../../store";
import { useNotionStatus, useTasks } from "./api";
import { TaskRow } from "./TaskRow";
import { UndoBar } from "./UndoBar";
import { useUndoableComplete } from "./useUndoableComplete";

const HOME_TASKS = 5;

export function TasksWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const source = useTasksSource();
  const status = useNotionStatus();
  const { data, isError, isPending, isFetching, refetch } = useTasks();
  const { onComplete, last, undo } = useUndoableComplete();

  if (!status.data?.connected || !source) {
    return (
      <Card title="Задачи" icon={CheckSquare}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Выберите источник заметок и задач в настройках →
        </button>
      </Card>
    );
  }

  const tasks = data?.tasks ?? [];

  return (
    <Card>
      <button
        onClick={() => setTab("tasks")}
        className="mb-1.5 flex w-full items-center gap-1.5 text-[12px] font-medium text-fg-muted hover:text-fg"
      >
        <CheckSquare className="size-3.5" strokeWidth={2.2} />
        <span className="truncate">{source.title}</span>
        {data && <span className="text-fg-subtle">· {tasks.length}</span>}
        <ChevronRight className="ml-auto size-3.5" />
      </button>

      {isError ? (
        <WidgetState kind="error" text="Не удалось загрузить задачи" onRetry={() => void refetch()} retrying={isFetching} />
      ) : isPending ? (
        <WidgetState kind="loading" />
      ) : data && tasks.length === 0 ? (
        <WidgetState kind="empty" text="Все задачи выполнены 🎉" />
      ) : (
        <div className="-mx-2 flex flex-col">
          <AnimatePresence initial={false}>
            {tasks.slice(0, HOME_TASKS).map((task) => (
              <TaskRow key={task.id} task={task} canComplete={!!data?.canComplete} onComplete={onComplete} compact />
            ))}
          </AnimatePresence>
        </div>
      )}
      {last && (
        <div className="mt-2">
          <UndoBar task={last} onUndo={undo} />
        </div>
      )}
    </Card>
  );
}
