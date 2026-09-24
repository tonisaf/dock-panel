import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface UpdateStatus {
  current: string;
  hasToken: boolean;
  available: { version: string; notes: string | null } | null;
  error: string | null;
}

const KEY = ["update-status"];

export function useUpdateStatus() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: KEY,
    queryFn: () => invoke<UpdateStatus>("update_status"),
    staleTime: Infinity,
  });

  // The background checker announces new versions.
  useEffect(() => {
    const un = listen("update:available", () => queryClient.invalidateQueries({ queryKey: KEY }));
    return () => {
      un.then((f) => f());
    };
  }, [queryClient]);

  return query;
}

export function useUpdateActions() {
  const queryClient = useQueryClient();
  const [progress, setProgress] = useState<number | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const un = listen<[number, number | null]>("update:progress", ({ payload: [done, total] }) => {
      setProgress(total ? Math.round((done / total) * 100) : null);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return {
    progress,
    installing,
    error,
    check: async () => {
      setError(null);
      queryClient.setQueryData(KEY, await invoke<UpdateStatus>("update_check"));
    },
    install: async () => {
      setError(null);
      setInstalling(true);
      try {
        // On success the installer closes and restarts the app.
        await invoke("update_install");
      } catch (e) {
        setError(String(e));
        setInstalling(false);
      }
    },
  };
}
