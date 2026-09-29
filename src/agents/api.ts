import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface Agent {
  /** `claude:<session id>` or `codex:<thread id>`. */
  id: string;
  kind: "claude" | "codex";
  name: string;
  /** Project folder name. */
  project: string;
  /** Where it runs: "Claude", "Codex", "VS Code", "терминал". */
  host: string;
  busy: boolean;
  /** Finished (or stopped to ask) and not looked at yet. */
  waiting: boolean;
  /** Unix ms of the last status change. */
  since: number;
  lastMessage: string | null;
  /** What a working session is doing: its latest tool call, or that it waits for an answer. */
  activity: string | null;
}

export interface AgentsState {
  agents: Agent[];
  notify: boolean;
}

export const AGENTS_KEY = ["agents"];
const KEY = AGENTS_KEY;

/** Refetched on `agents:changed` (see useDataEvents), not polled. */
export function useAgents() {
  return useQuery({
    queryKey: KEY,
    queryFn: () => invoke<AgentsState>("agents_list"),
    staleTime: Infinity,
  });
}

/** Re-renders once a minute, for "5 мин"-style labels on data that doesn't change. */
export function useMinuteTick() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 60_000);
    return () => clearInterval(id);
  }, []);
}

export function useAgentActions() {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries({ queryKey: KEY });
  return {
    focus: async (id: string) => {
      try {
        await invoke("agents_focus", { id });
      } finally {
        await refresh();
      }
    },
    dismiss: async (id: string | null) => {
      await invoke("agents_dismiss", { id });
      await refresh();
    },
    setNotify: async (on: boolean) => {
      await invoke("agents_set_notify", { on });
      await refresh();
    },
  };
}

/** "только что", "5 мин", "2 ч", "3 дн". */
export function ago(ms: number) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "только что";
  if (s < 3600) return `${Math.floor(s / 60)} мин`;
  if (s < 86_400) return `${Math.floor(s / 3600)} ч`;
  return `${Math.floor(s / 86_400)} дн`;
}
