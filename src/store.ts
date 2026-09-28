import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { deskWidget } from "./lib/desktop";
import { AppWindow, CalendarDays, CheckSquare, LayoutGrid, Mail, Settings, Sparkles, StickyNote, type LucideIcon } from "lucide-react";

export type TabId = "home" | "apps" | "tasks" | "mail" | "calendar" | "ai" | "notes" | "settings";

export const TABS: { id: TabId; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "Главная", icon: LayoutGrid },
  { id: "apps", label: "Приложения", icon: AppWindow },
  { id: "tasks", label: "Задачи", icon: CheckSquare },
  { id: "mail", label: "Почта", icon: Mail },
  { id: "calendar", label: "Календарь", icon: CalendarDays },
  { id: "ai", label: "AI", icon: Sparkles },
  { id: "notes", label: "Заметки", icon: StickyNote },
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
  /** A note to open in the notes tab (from search or the widget); "new" opens the composer. */
  noteToOpen: string | null;
  /** Pinned: the panel stays open until hidden explicitly. */
  pinned: boolean;
  /** Full screen: home on the left, the other tabs on the right. */
  full: boolean;
  /** The right side's tab in full screen: the last tab other than home. */
  sideTab: TabId;
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
  /** Switches to the notes tab and opens the note. */
  openNote: (id: string | null) => void;
  /** Closes the panel even when pinned: the hide button, the hotkey, the tray. */
  hide: () => void;
  setPinned: (pinned: boolean) => void;
  setFull: (full: boolean) => void;
}

export const usePanelStore = create<PanelState>((set, get) => ({
  open: false,
  tab: "home",
  query: "",
  selected: 0,
  menu: null,
  mailToOpen: null,
  noteToOpen: null,
  pinned: false,
  full: false,
  sideTab: "apps",
  setOpen: (open) => {
    // A widget on the desktop has no panel of its own to close.
    if (deskWidget) return;
    if (!open && get().pinned) return;
    set(open ? { open } : { open, menu: null });
  },
  hide: () => set({ open: false, menu: null }),
  setPinned: (pinned) => {
    set({ pinned });
    invoke("panel_set_pinned", { on: pinned }).catch(console.error);
  },
  setTab: (tab) => {
    // From a widget on the desktop: open the panel there.
    if (deskWidget) invoke("panel_open", { tab }).catch(console.error);
    else set(tab === "home" ? { tab } : { tab, sideTab: tab });
  },
  setFull: (full) => {
    set({ full });
    invoke("panel_set_fullscreen", { on: full }).catch(console.error);
  },
  setQuery: (query) => set({ query, selected: 0 }),
  setSelected: (selected) => set({ selected }),
  setMenu: (menu) => set({ menu }),
  setMailToOpen: (mailToOpen) => set({ mailToOpen }),
  openNote: (noteToOpen) => {
    if (deskWidget) {
      invoke("panel_open", { tab: "notes", note: noteToOpen }).catch(console.error);
      return;
    }
    set({ noteToOpen, query: "", selected: 0 });
    if (noteToOpen) get().setTab("notes");
  },
}));
