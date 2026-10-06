import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { usePrefs } from "./prefs";

export interface Integrations { llm: "lmstudio" | "ollama"; notes: "notion" | "obsidian" | "local"; vault: string; tasks: "inherit" | "notion" | "obsidian" }
export const useIntegrations = create<Integrations & { loaded: boolean }>(() => ({ llm: "lmstudio", notes: "notion", vault: "", tasks: "inherit", loaded: false }));
const ready = invoke<Integrations>("integrations_settings").then((s) => useIntegrations.setState({ ...s, loaded: true }));
// Browser-only previews have no native backend.
void ready.catch(() => {});
void listen<Integrations>("integrations:changed", (event) => useIntegrations.setState({ ...event.payload, loaded: true })).catch(() => {});
export async function saveIntegrations(settings: Integrations) {
  const saved = await invoke<Integrations>("integrations_save", { settings });
  useIntegrations.setState({ ...saved, loaded: true });
  await emit("integrations:changed", saved);
}
export async function dataInvoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  await ready;
  if (useIntegrations.getState().notes === "local" && command.startsWith("notes_")) return invoke<T>("local_notes_command", { op: command, args });
  const settings = useIntegrations.getState();
  const provider = command.startsWith("notion_") ? tasksProvider(settings) : settings.notes;
  return provider === "obsidian"
    ? invoke<T>("obsidian_command", { op: command, args })
    : invoke<T>(command, args);
}
export const tasksProvider = (s: Integrations) => s.tasks === "inherit" ? s.notes === "obsidian" ? "obsidian" : "notion" : s.tasks;
export function useTasksSource() {
  const settings = useIntegrations();
  const notion = usePrefs((s) => s.notionSource);
  if (!settings.loaded) return null;
  return tasksProvider(settings) === "obsidian" ? (settings.vault ? { id: `obsidian:${settings.vault}`, title: "Obsidian", databaseId: undefined } : null) : notion;
}
export const llmName = () => useIntegrations.getState().llm === "ollama" ? "Ollama" : "LM Studio";
