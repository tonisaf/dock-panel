import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export type Phase = "focus" | "short" | "long";

export interface PomodoroSettings {
  focusMin: number;
  shortMin: number;
  longMin: number;
  /** A long break after this many focus sessions. */
  longEvery: number;
  /** The next phase starts by itself. */
  autoStart: boolean;
}

export interface PomodoroState {
  phase: Phase;
  running: boolean;
  /** Unix ms, while running. */
  endsAt: number | null;
  remainingMs: number;
  totalMs: number;
  doneInCycle: number;
  today: number;
  settings: PomodoroSettings;
}

const KEY = ["pomodoro"];

/** The backend keeps time and pushes every change; the countdown itself is drawn here. */
export function usePomodoro() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const un = listen<PomodoroState>("pomodoro:changed", (e) => queryClient.setQueryData(KEY, e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [queryClient]);
  return useQuery({ queryKey: KEY, queryFn: () => invoke<PomodoroState>("pomodoro_state"), staleTime: Infinity });
}

/** Milliseconds left, ticking while the timer runs. */
export function useTimeLeft(s: PomodoroState | undefined) {
  const [now, setNow] = useState(Date.now());
  const running = !!s?.running;
  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [running]);
  if (!s) return 0;
  return s.running && s.endsAt != null ? Math.max(0, s.endsAt - now) : s.remainingMs;
}

export function usePomodoroActions() {
  const queryClient = useQueryClient();
  const call = async (cmd: string, args?: Record<string, unknown>) =>
    queryClient.setQueryData(KEY, await invoke<PomodoroState>(cmd, args));
  return {
    start: () => call("pomodoro_start"),
    pause: () => call("pomodoro_pause"),
    reset: () => call("pomodoro_reset"),
    skip: () => call("pomodoro_skip"),
    setPhase: (phase: Phase) => call("pomodoro_set_phase", { phase }),
    setSettings: (settings: PomodoroSettings) => call("pomodoro_set_settings", { settings }),
  };
}

/** "24:59". */
export function clock(ms: number) {
  const secs = Math.ceil(ms / 1000);
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}
