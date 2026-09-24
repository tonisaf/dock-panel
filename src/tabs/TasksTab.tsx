import { useState } from "react";
import { AnimatePresence } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckSquare, ExternalLink, Loader2, Plus, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { EmptyState } from "../components/Card";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { useCreateTask, useNotionStatus, useTasks } from "../widgets/tasks/api";
import { TaskRow } from "../widgets/tasks/TaskRow";
import { UndoBar } from "../widgets/tasks/UndoBar";
import { useUndoableComplete } from "../widgets/tasks/useUndoableComplete";

function QuickAdd() {
  const [title, setTitle] = useState("");
  const create = useCreateTask();

  const submit = () => {
    const t = title.trim();
    if (!t || create.isPending) return;
    create.mutate(t, { onSuccess: () => setTitle("") });
  };

  return (
    <div>
      <label className="flex h-10 items-center gap-2 rounded-xl border border-stroke bg-surface px-3 focus-within:border-accent/50">
        {create.isPending ? (
          <Loader2 className="size-4 animate-spin text-fg-subtle" />
        ) : (
          <Plus className="size-4 text-fg-subtle" />
        )}
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit();
            }
          }}
          placeholder="Новая задача, Enter — добавить"
          spellCheck={false}
          className="h-full flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-fg-subtle"
        />
      </label>
      {create.isError && <p className="mt-1 px-1 text-[12px] text-warn">{String(create.error)}</p>}
    </div>
  );
}

export function TasksTab() {
  const source = usePrefs((s) => s.notionSource);
  const setTab = usePanelStore((s) => s.setTab);
  const status = useNotionStatus();
  const { data, isPending, isError, error, refetch, isFetching } = useTasks();
  const { onComplete, last, undo, error: completeError } = useUndoableComplete();

  if (!status.data?.connected || !source) {
    return (
      <div className="flex h-full flex-col items-center justify-center">
        <EmptyState
          icon={CheckSquare}
          title="Задачи из Notion"
          text={
            status.data?.connected
              ? "Выберите базу задач в настройках."
              : "Подключите Notion в настройках: понадобится токен внутренней интеграции."
          }
        />
        <button
          onClick={() => setTab("settings")}
          className="-mt-12 rounded-lg border border-stroke px-3 py-1.5 text-[12.5px] text-fg-muted hover:bg-ink/8 hover:text-fg"
        >
          Открыть настройки
        </button>
      </div>
    );
  }

  const tasks = data?.tasks ?? [];

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex items-center justify-between px-1">
        <div className="min-w-0">
          <div className="truncate text-[15px] font-semibold">{source.title}</div>
          <div className="text-[12px] text-fg-subtle">
            {isPending ? "Загрузка…" : `${tasks.length} открыт${tasks.length === 1 ? "ая" : "ых"}`}
          </div>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => refetch()}
            title="Обновить"
            className="grid size-8 place-items-center rounded-lg text-fg-subtle hover:bg-ink/8 hover:text-fg"
          >
            <RefreshCw className={clsx("size-4", isFetching && "animate-spin")} />
          </button>
          <button
            onClick={() => {
              openUrl(`https://www.notion.so/${(source.databaseId ?? source.id).replace(/-/g, "")}`).catch(console.error);
              usePanelStore.getState().setOpen(false);
            }}
            title="Открыть в Notion"
            className="grid size-8 place-items-center rounded-lg text-fg-subtle hover:bg-ink/8 hover:text-fg"
          >
            <ExternalLink className="size-4" />
          </button>
        </div>
      </div>

      <QuickAdd />

      {(isError || completeError) && (
        <p className="px-1 text-[12px] leading-relaxed text-warn">{String(completeError ?? error)}</p>
      )}

      <div className="scroll-area -mx-1 min-h-0 flex-1 px-1">
        {!isPending && tasks.length === 0 && !isError ? (
          <p className="px-2 pt-6 text-center text-[13px] text-fg-subtle">Все задачи выполнены 🎉</p>
        ) : (
          <div className="flex flex-col pb-2">
            <AnimatePresence initial={false}>
              {tasks.map((task) => (
                <TaskRow key={task.id} task={task} canComplete={!!data?.canComplete} onComplete={onComplete} />
              ))}
            </AnimatePresence>
          </div>
        )}
      </div>

      <UndoBar task={last} onUndo={undo} />
    </div>
  );
}
