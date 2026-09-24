import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface LimitWindow {
  kind: string;
  usedPercent: number;
  /** Unix seconds. */
  resetsAt: number;
}

export interface Snapshot {
  /** Unix ms of when the source recorded the data. */
  updatedAt: number;
  plan: string | null;
  windows: LimitWindow[];
}

export interface ClaudeWebStatus {
  enabled: boolean;
  needsLogin: boolean;
  fetching: boolean;
  error: string | null;
}

export interface AiLimits {
  claude: Snapshot | null;
  /** Claude Code status line integration is installed. */
  claudeConnected: boolean;
  /** Experimental claude.ai sign-in. */
  claudeWeb: ClaudeWebStatus;
  /** Reset notifications, per provider. */
  alerts: { claude: boolean; codex: boolean };
  codex: Snapshot | null;
}

export function useAiLimits() {
  return useQuery({
    queryKey: ["ai-limits"],
    queryFn: () => invoke<AiLimits>("ai_limits"),
    refetchInterval: 15_000,
    staleTime: 0,
  });
}

const WINDOW_ORDER = ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet", "spend_limit"];

/** claude.ai `rate_limit_tier`, e.g. `default_claude_max_5x`, as a badge. */
export function claudePlanLabel(tier: string | null) {
  if (!tier) return null;
  const t = tier.toLowerCase();
  if (t.includes("max_20x")) return "MAX 20×";
  if (t.includes("max_5x")) return "MAX 5×";
  if (t.includes("max")) return "MAX";
  if (t.includes("pro")) return "PRO";
  if (t.includes("team")) return "TEAM";
  return null;
}

export function windowLabel(kind: string) {
  if (kind === "five_hour") return "5 часов";
  if (kind === "seven_day") return "Неделя";
  if (kind === "spend_limit") return "Расходы";
  if (kind === "seven_day_opus") return "Opus";
  if (kind === "seven_day_sonnet") return "Sonnet";
  // claude.ai sometimes returns extra windows under internal code names.
  if (!kind.startsWith("window_")) return "Доп. лимит";
  const minutes = Number(kind.replace("window_", ""));
  return minutes >= 1440 ? `${Math.round(minutes / 1440)} дн` : `${Math.round(minutes / 60)} ч`;
}

const orderOf = (kind: string) => {
  const i = WINDOW_ORDER.indexOf(kind);
  return i === -1 ? WINDOW_ORDER.length : i;
};

const isKnown = (kind: string) => WINDOW_ORDER.includes(kind) || kind.startsWith("window_");

/**
 * A window whose reset time has passed is empty again, however old the
 * snapshot. Unrecognised windows are shown only once something is used.
 */
export function effectiveWindows(snapshot: Snapshot, now = Date.now()) {
  return [...snapshot.windows]
    .sort((a, b) => orderOf(a.kind) - orderOf(b.kind))
    .map((w) => {
      const reset = w.resetsAt * 1000 <= now;
      return { ...w, usedPercent: reset ? 0 : w.usedPercent, reset };
    })
    .filter((w) => isKnown(w.kind) || w.usedPercent > 0);
}

export function formatIn(unixSeconds: number, now = Date.now()) {
  const min = Math.max(0, Math.round((unixSeconds * 1000 - now) / 60_000));
  if (min < 60) return `${min} мин`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} ч ${min % 60} мин`;
  return `${Math.floor(h / 24)} дн ${h % 24} ч`;
}

export function formatAgo(ms: number, now = Date.now()) {
  const min = Math.round((now - ms) / 60_000);
  if (min < 1) return "только что";
  if (min < 60) return `${min} мин назад`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h} ч назад`;
  return `${Math.round(h / 24)} дн назад`;
}

export function formatResetAt(unixSeconds: number) {
  return new Date(unixSeconds * 1000).toLocaleString("ru-RU", {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function levelClass(percent: number) {
  if (percent >= 90) return "bg-red-400";
  if (percent >= 70) return "bg-amber-400";
  return "bg-accent";
}
