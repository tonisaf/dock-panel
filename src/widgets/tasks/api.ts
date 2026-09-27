import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usePrefs } from "../../lib/prefs";

export interface Badge {
  name: string;
  /** Notion color name: default, gray, brown, orange, yellow, green, blue, purple, pink, red. */
  color: string;
}

export interface Task {
  id: string;
  url: string;
  title: string;
  status: Badge | null;
  inProgress: boolean;
  due: string | null;
  priority: Badge | null;
  tag: Badge | null;
}

export interface TaskList {
  tasks: Task[];
  canComplete: boolean;
}

export interface NotionStatus {
  connected: boolean;
  workspace: string | null;
  error: string | null;
}

export function useNotionStatus() {
  return useQuery({
    queryKey: ["notion-status"],
    queryFn: () => invoke<NotionStatus>("notion_status"),
    staleTime: 5 * 60_000,
  });
}

export function useTasks() {
  const source = usePrefs((s) => s.notionSource);
  return useQuery({
    queryKey: ["notion-tasks", source?.id],
    queryFn: () => invoke<TaskList>("notion_tasks", { sourceId: source!.id }),
    enabled: !!source,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
}

/** Optimistically drops the task; `onDone` gets the previous status for undo. */
export function useCompleteTask() {
  const queryClient = useQueryClient();
  const source = usePrefs((s) => s.notionSource);
  const key = ["notion-tasks", source?.id];

  return useMutation({
    mutationFn: (task: Task) => invoke("notion_complete", { sourceId: source!.id, pageId: task.id }),
    onMutate: async (task) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<TaskList>(key);
      if (previous) {
        queryClient.setQueryData<TaskList>(key, { ...previous, tasks: previous.tasks.filter((t) => t.id !== task.id) });
      }
      return { previous };
    },
    onError: (_e, _task, ctx) => {
      if (ctx?.previous) queryClient.setQueryData(key, ctx.previous);
    },
  });
}

export async function restoreTask(sourceId: string, task: Task) {
  await invoke("notion_restore", { sourceId, pageId: task.id, status: task.status?.name ?? null });
}

export function useCreateTask() {
  const queryClient = useQueryClient();
  const source = usePrefs((s) => s.notionSource);
  const key = ["notion-tasks", source?.id];

  return useMutation({
    mutationFn: (title: string) => invoke<Task>("notion_create", { sourceId: source!.id, title }),
    onSuccess: (task) => {
      queryClient.setQueryData<TaskList>(key, (prev) => (prev ? { ...prev, tasks: [task, ...prev.tasks] } : prev));
      queryClient.invalidateQueries({ queryKey: key });
    },
  });
}

const NOTION_COLOR_NAMES = new Set(["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"]);

/** A Notion colour name as CSS, per theme (see `--notion-*` in index.css). */
export const notionColor = (color: string) => `var(--notion-${NOTION_COLOR_NAMES.has(color) ? color : "default"})`;

/** "Сегодня", "Завтра", "25 сен", with overdue flagged. */
export function describeDue(due: string, now = new Date()) {
  const hasTime = due.length > 10;
  const date = new Date(hasTime ? due : `${due}T00:00:00`);
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOfDay(date) - startOfDay(now)) / 86_400_000);
  const time = hasTime ? " " + date.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" }) : "";

  let label: string;
  if (days === 0) label = "Сегодня";
  else if (days === 1) label = "Завтра";
  else if (days === -1) label = "Вчера";
  else label = date.toLocaleDateString("ru-RU", { day: "numeric", month: "short" }).replace(".", "");

  const overdue = hasTime ? date.getTime() < now.getTime() : days < 0;
  return { label: label + time, overdue, soon: !overdue && days <= 1 };
}

export interface TaskSchema {
  statuses: Badge[];
  priorities: Badge[];
  tags: Badge[];
  hasStatus: boolean;
  hasDue: boolean;
  hasPriority: boolean;
  hasTag: boolean;
}

export function useTaskSchema(enabled: boolean) {
  const source = usePrefs((s) => s.notionSource);
  return useQuery({
    queryKey: ["notion-task-schema", source?.id],
    queryFn: () => invoke<TaskSchema>("notion_task_schema", { sourceId: source!.id }),
    enabled: enabled && !!source,
    staleTime: 5 * 60_000,
  });
}

/** Fields to change; `null` clears due, priority or tag. */
export interface TaskChange {
  title?: string;
  status?: string;
  due?: string | null;
  priority?: string | null;
  tag?: string | null;
}

/** Edits and deletion, shown in the list at once and put right by Notion's answer. */
export function useTaskActions() {
  const queryClient = useQueryClient();
  const source = usePrefs((s) => s.notionSource);
  const key = ["notion-tasks", source?.id];
  const patch = (fn: (tasks: Task[]) => Task[]) =>
    queryClient.setQueryData<TaskList>(key, (prev) => (prev ? { ...prev, tasks: fn(prev.tasks) } : prev));

  return {
    update: async (task: Task, change: TaskChange) => {
      const updated = await invoke<Task>("notion_update", { sourceId: source!.id, pageId: task.id, change });
      patch((tasks) => tasks.map((t) => (t.id === task.id ? updated : t)));
      // A status in the "Complete" group takes the task off the list.
      if (change.status) queryClient.invalidateQueries({ queryKey: key });
      return updated;
    },
    remove: async (task: Task) => {
      patch((tasks) => tasks.filter((t) => t.id !== task.id));
      try {
        await invoke("notion_delete", { pageId: task.id });
      } catch (e) {
        queryClient.invalidateQueries({ queryKey: key });
        throw e;
      }
    },
  };
}
