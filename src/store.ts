import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { AppWindow, CheckSquare, LayoutGrid, Mail, Settings, Sparkles, type LucideIcon } from "lucide-react";

export type TabId = "home" | "apps" | "tasks" | "mail" | "ai" | "settings";

export const TABS: { id: TabId; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "Главная", icon: LayoutGrid },
  { id: "apps", label: "Приложения", icon: AppWindow },
  { id: "tasks", label: "Задачи", icon: CheckSquare },
  { id: "mail", label: "Почта", icon: Mail },
  { id: "ai", label: "AI", icon: Sparkles },
  { id: "settings", label: "Настройки", icon: Settings },
];

/** The letter summary a notification carries (see `Summary` in mail/api). */
export interface MailToOpen {
  account: string;
  uid: number;
  fromName: string;
  fromEmail: string;
  subject: string;
  date: number;
  unread: boolean;
  flagged: boolean;
}

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
  /** A letter to open in the mail tab, e.g. from a clicked notification. */
  mailToOpen: MailToOpen | null;
  /** Pinned: the panel stays open until hidden explicitly. */
  pinned: boolean;
  /**
   * Opening always works; closing is the panel's own "done here" (Esc, after
   * launching something) and is ignored while pinned. Use `hide` to force it.
   */
  setOpen: (open: boolean) => void;
  setTab: (tab: TabId) => void;
  setQuery: (query: string) => void;
  setSelected: (selected: number) => void;
  setMenu: (menu: ContextMenuState | null) => void;
  setMailToOpen: (letter: MailToOpen | null) => void;
  /** Closes the panel even when pinned: the hide button, the hotkey, the tray. */
  hide: () => void;
  setPinned: (pinned: boolean) => void;
}

export const usePanelStore = create<PanelState>((set, get) => ({
  open: false,
  tab: "home",
  query: "",
  selected: 0,
  menu: null,
  mailToOpen: null,
  pinned: false,
  setOpen: (open) => {
    if (!open && get().pinned) return;
    set(open ? { open } : { open, menu: null });
  },
  hide: () => set({ open: false, menu: null }),
  setPinned: (pinned) => {
    set({ pinned });
    invoke("panel_set_pinned", { on: pinned }).catch(console.error);
  },
  setTab: (tab) => set({ tab }),
  setQuery: (query) => set({ query, selected: 0 }),
  setSelected: (selected) => set({ selected }),
  setMenu: (menu) => set({ menu }),
  setMailToOpen: (mailToOpen) => set({ mailToOpen }),
}));
