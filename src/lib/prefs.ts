import { create } from "zustand";
import { load, type Store } from "@tauri-apps/plugin-store";

export interface Usage {
  count: number;
  last: number;
}

export interface WeatherLocation {
  name: string;
  /** Region and country, for disambiguation in the UI. */
  detail: string;
  latitude: number;
  longitude: number;
}

export interface NotionSource {
  /** Data source id. */
  id: string;
  title: string;
  databaseId?: string | null;
}

export type ThemeMode = "system" | "dark" | "light";

interface PrefsState {
  pinned: string[];
  usage: Record<string, Usage>;
  location: WeatherLocation | null;
  notionSource: NotionSource | null;
  theme: ThemeMode;
  /** Use the Windows accent color instead of the built-in blue. */
  systemAccent: boolean;
  /** Home widget ids in display order; unknown/new widgets go last. */
  widgetOrder: string[];
  hiddenWidgets: string[];
  /** Hand-arranged widget columns, keyed by column count; none means auto (masonry). */
  widgetColumns: Record<string, string[][]>;
  /** Width of the mail list while a letter is open next to it; null for the default. */
  mailListWidth: number | null;
  /** Collapsed state of collapsible widgets, by id; a widget picks its own default. */
  collapsedWidgets: Record<string, boolean>;
  togglePin: (id: string) => void;
  /** Pins ids that are not pinned yet, keeping their order. */
  pinMany: (ids: string[]) => void;
  recordLaunch: (id: string) => void;
  setLocation: (location: WeatherLocation | null) => void;
  setNotionSource: (source: NotionSource | null) => void;
  setTheme: (theme: ThemeMode) => void;
  setSystemAccent: (on: boolean) => void;
  /** `columns` saves an arrangement for that many columns; null drops all of them (back to auto). */
  setWidgetLayout: (order: string[], hidden: string[], columns: string[][] | null) => void;
  setMailListWidth: (width: number) => void;
  setWidgetCollapsed: (id: string, collapsed: boolean) => void;
}

/** User preferences, persisted to `prefs.json` in the app data dir. */
let store: Store | null = null;

function persist(key: string, value: unknown) {
  store?.set(key, value).catch((e) => console.error("prefs save failed", e));
}

export const usePrefs = create<PrefsState>((set, get) => ({
  pinned: [],
  usage: {},
  location: null,
  notionSource: null,
  theme: "system",
  systemAccent: true,
  widgetOrder: [],
  hiddenWidgets: [],
  widgetColumns: {},
  mailListWidth: null,
  collapsedWidgets: {},
  togglePin: (id) => {
    const current = get().pinned;
    const pinned = current.includes(id) ? current.filter((p) => p !== id) : [...current, id];
    set({ pinned });
    persist("pinned", pinned);
  },
  pinMany: (ids) => {
    const current = get().pinned;
    const added = ids.filter((id, i) => !current.includes(id) && ids.indexOf(id) === i);
    if (added.length === 0) return;
    const pinned = [...current, ...added];
    set({ pinned });
    persist("pinned", pinned);
  },
  recordLaunch: (id) => {
    const prev = get().usage[id];
    const usage = { ...get().usage, [id]: { count: (prev?.count ?? 0) + 1, last: Date.now() } };
    set({ usage });
    persist("usage", usage);
  },
  setLocation: (location) => {
    set({ location });
    persist("location", location);
  },
  setNotionSource: (notionSource) => {
    set({ notionSource });
    persist("notionSource", notionSource);
  },
  setTheme: (theme) => {
    set({ theme });
    persist("theme", theme);
  },
  setSystemAccent: (systemAccent) => {
    set({ systemAccent });
    persist("systemAccent", systemAccent);
  },
  setMailListWidth: (mailListWidth) => {
    set({ mailListWidth });
    persist("mailListWidth", mailListWidth);
  },
  setWidgetCollapsed: (id, collapsed) => {
    const collapsedWidgets = { ...get().collapsedWidgets, [id]: collapsed };
    set({ collapsedWidgets });
    persist("collapsedWidgets", collapsedWidgets);
  },
  setWidgetLayout: (widgetOrder, hiddenWidgets, columns) => {
    const widgetColumns = columns ? { ...get().widgetColumns, [columns.length]: columns } : {};
    set({ widgetOrder, hiddenWidgets, widgetColumns });
    persist("widgetOrder", widgetOrder);
    persist("hiddenWidgets", hiddenWidgets);
    persist("widgetColumns", widgetColumns);
  },
}));

load("prefs.json", { defaults: {}, autoSave: 300 })
  .then(async (s) => {
    store = s;
    usePrefs.setState({
      pinned: (await s.get<string[]>("pinned")) ?? [],
      usage: (await s.get<Record<string, Usage>>("usage")) ?? {},
      location: (await s.get<WeatherLocation>("location")) ?? null,
      notionSource: (await s.get<NotionSource>("notionSource")) ?? null,
      theme: (await s.get<ThemeMode>("theme")) ?? "system",
      systemAccent: (await s.get<boolean>("systemAccent")) ?? true,
      widgetOrder: (await s.get<string[]>("widgetOrder")) ?? [],
      hiddenWidgets: (await s.get<string[]>("hiddenWidgets")) ?? [],
      widgetColumns: (await s.get<Record<string, string[][]>>("widgetColumns")) ?? {},
      mailListWidth: (await s.get<number>("mailListWidth")) ?? null,
      collapsedWidgets: (await s.get<Record<string, boolean>>("collapsedWidgets")) ?? {},
    });
  })
  .catch((e) => console.error("prefs load failed", e));
