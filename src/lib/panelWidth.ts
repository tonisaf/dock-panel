import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export const MIN_WIDTH = 360;
export const MAX_WIDTH = 1280;

/** Widths that fit exactly 1, 2 and 3 widget columns (340px each, 12px gaps, 16px padding). */
export const WIDTH_PRESETS = [
  { label: "1 колонка", width: 440 },
  { label: "2 колонки", width: 760 },
  { label: "3 колонки", width: 1120 },
];

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
}

const KEY = ["panel-settings"];
const DEFAULTS: PanelSettings = { width: 440, edge: "left", shortcut: "Ctrl+Space", taskbarButton: true, taskbarPlayer: true, taskbarMail: true, taskbarTasks: true };

export function usePanelSettings() {
  const queryClient = useQueryClient();
  const { data = DEFAULTS } = useQuery({
    queryKey: KEY,
    queryFn: () => invoke<PanelSettings>("panel_settings"),
    staleTime: Infinity,
  });
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
