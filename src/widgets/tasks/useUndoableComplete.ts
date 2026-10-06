import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTasksSource } from "../../lib/integrations";
import { restoreTask, useCompleteTask, type Task } from "./api";

const UNDO_MS = 5000;

/** Completes a task and offers a few seconds to take it back. */
export function useUndoableComplete() {
  const complete = useCompleteTask();
  const queryClient = useQueryClient();
  const source = useTasksSource();
  const [last, setLast] = useState<Task | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  const onComplete = (task: Task) => {
    setError(null);
    complete.mutate(task, {
      onSuccess: () => {
        setLast(task);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setLast(null), UNDO_MS);
      },
      onError: (e) => setError(String(e)),
    });
  };

  const undo = async () => {
    if (!last || !source) return;
    const task = last;
    setLast(null);
    try {
      await restoreTask(source.id, task);
    } catch (e) {
      setError(String(e));
    }
    queryClient.invalidateQueries({ queryKey: ["notion-tasks", source.id] });
    queryClient.invalidateQueries({ queryKey: ["notes"] });
  };

  return { onComplete, last, undo, error };
}
