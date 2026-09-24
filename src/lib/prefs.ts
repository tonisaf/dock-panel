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
  togglePin: (id: string) => void;
  recordLaunch: (id: string) => void;
  setLocation: (location: WeatherLocation | null) => void;
  setNotionSource: (source: NotionSource | null) => void;
  setTheme: (theme: ThemeMode) => void;
  setSystemAccent: (on: boolean) => void;
  setWidgetLayout: (order: string[], hidden: string[]) => void;
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
  togglePin: (id) => {
    const current = get().pinned;
    const pinned = current.includes(id) ? current.filter((p) => p !== id) : [...current, id];
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
  setWidgetLayout: (widgetOrder, hiddenWidgets) => {
    set({ widgetOrder, hiddenWidgets });
    persist("widgetOrder", widgetOrder);
    persist("hiddenWidgets", hiddenWidgets);
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
    });
  })
  .catch((e) => console.error("prefs load failed", e));
