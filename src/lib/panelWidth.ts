import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

/** Must match MIN_WIDTH / MAX_WIDTH in panel.rs. */
export const MIN_WIDTH = 360;
export const MAX_WIDTH = 2400;

/** Widget width, logical px: the user's choice, its range and default. */
export const WIDGET_MIN = 300;
export const WIDGET_MAX = 700;
export const WIDGET_DEFAULT = 500;
/** Gap between widget columns (`gap-3`). */
export const WIDGET_GAP = 12;
/** Panel width around the columns: padding and the scrollbar. */
const CHROME = 40;

/** The panel width that fits exactly `columns` widgets `widget` wide. */
export function panelWidthFor(columns: number, widget: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, columns * widget + (columns - 1) * WIDGET_GAP + CHROME));
}

/** How many widgets `widget` wide a panel this wide fits side by side. */
export function columnsIn(panel: number, widget: number) {
  return Math.max(1, Math.floor((panel - CHROME + WIDGET_GAP) / (widget + WIDGET_GAP)));
}

/** Widths that fit exactly 1, 2 and 3 widget columns. */
export function widthPresets(widget: number) {
  return ["1 колонка", "2 колонки", "3 колонки"].map((label, i) => ({ label, width: panelWidthFor(i + 1, widget) }));
}

export type Edge = "left" | "right";

export interface PanelSettings {
  width: number;
  edge: Edge;
  /** Accelerator string, e.g. "Ctrl+Space". */
  shortcut: string;
  /** Dock Panel button at the left end of the taskbar. */
  taskbarButton: boolean;
  /** Mini player next to that button. */
  taskbarPlayer: boolean;
  /** Unread-mail counter next to that button. */
  taskbarMail: boolean;
  /** Counter of Google tasks due today next to that button. */
  taskbarTasks: boolean;
  /** Counter of Claude / Codex sessions waiting for the user. */
  taskbarAgents: boolean;
  /** Discord microphone while in a voice channel. */
  taskbarMic: boolean;
  /** Pomodoro countdown while a phase is under way. */
  taskbarPomodoro: boolean;
}

const KEY = ["panel-settings"];
/** Dispatched on `window` when the settings change behind the cache's back. */
export const PANEL_SETTINGS_CHANGED = "panel-settings-changed";
const DEFAULTS: PanelSettings = { width: 540, edge: "left", shortcut: "Ctrl+Space", taskbarButton: true, taskbarPlayer: true, taskbarMail: true, taskbarTasks: true, taskbarAgents: true, taskbarMic: true, taskbarPomodoro: true };

export function usePanelSettings() {
  const queryClient = useQueryClient();
  const { data = DEFAULTS } = useQuery({
    queryKey: KEY,
    queryFn: () => invoke<PanelSettings>("panel_settings"),
    staleTime: Infinity,
  });
  useEffect(() => {
    const refetch = () => queryClient.invalidateQueries({ queryKey: KEY });
    window.addEventListener(PANEL_SETTINGS_CHANGED, refetch);
    return () => window.removeEventListener(PANEL_SETTINGS_CHANGED, refetch);
  }, [queryClient]);
  const update = (s: PanelSettings) => queryClient.setQueryData(KEY, s);

  return {
    ...data,
    setEdge: async (edge: Edge) => update(await invoke<PanelSettings>("panel_set_edge", { edge })),
    /** Throws the backend's message if the combination is taken. */
    setShortcut: async (shortcut: string) => update(await invoke<PanelSettings>("panel_set_shortcut", { shortcut })),
    setTaskbarButton: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_button", { on })),
    setTaskbarPlayer: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_player", { on })),
    setTaskbarMail: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_mail", { on })),
    setTaskbarTasks: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_tasks", { on })),
    setTaskbarAgents: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_agents", { on })),
    setTaskbarMic: async (on: boolean) => update(await invoke<PanelSettings>("panel_set_taskbar_mic", { on })),
    setTaskbarPomodoro: async (on: boolean) =>
      update(await invoke<PanelSettings>("panel_set_taskbar_pomodoro", { on })),
    suspendShortcut: (suspend: boolean) => invoke("panel_suspend_shortcut", { suspend }),
  };
}

export function usePanelWidth() {
  const queryClient = useQueryClient();
  const { width, edge } = usePanelSettings();

  /** Resizes live; pass `persist` once the user settles on a value. */
  const setWidth = async (next: number, persist: boolean) => {
    const applied = await invoke<number>("panel_set_width", { width: Math.round(next), persist });
    queryClient.setQueryData<PanelSettings>(KEY, (prev) => ({ ...(prev ?? DEFAULTS), width: applied }));
    return applied;
  };

  return { width, edge, setWidth };
}
