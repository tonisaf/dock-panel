import { create } from "zustand";
import { AppWindow, CheckSquare, LayoutGrid, Settings, Sparkles, type LucideIcon } from "lucide-react";

export type TabId = "home" | "apps" | "tasks" | "ai" | "settings";

export const TABS: { id: TabId; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "Главная", icon: LayoutGrid },
  { id: "apps", label: "Приложения", icon: AppWindow },
  { id: "tasks", label: "Задачи", icon: CheckSquare },
  { id: "ai", label: "AI", icon: Sparkles },
  { id: "settings", label: "Настройки", icon: Settings },
];

export interface ContextMenuState {
  appId: string;
  x: number;
  y: number;
}

interface PanelState {
  /** Content visibility; the window itself hides after the exit animation. */
  open: boolean;
  tab: TabId;
  query: string;
  /** Highlighted search result, driven by the arrow keys. */
  selected: number;
  menu: ContextMenuState | null;
  setOpen: (open: boolean) => void;
  setTab: (tab: TabId) => void;
  setQuery: (query: string) => void;
  setSelected: (selected: number) => void;
  setMenu: (menu: ContextMenuState | null) => void;
}

export const usePanelStore = create<PanelState>((set) => ({
  open: false,
  tab: "home",
  query: "",
  selected: 0,
  menu: null,
  setOpen: (open) => set(open ? { open } : { open, menu: null }),
  setTab: (tab) => set({ tab }),
  setQuery: (query) => set({ query, selected: 0 }),
  setSelected: (selected) => set({ selected }),
  setMenu: (menu) => set({ menu }),
}));
