import { create } from "zustand";
import { load, type Store } from "@tauri-apps/plugin-store";
import { invoke } from "@tauri-apps/api/core";
import { PANEL_SETTINGS_CHANGED, WIDGET_DEFAULT, columnsIn, panelWidthFor } from "./panelWidth";
import { deskWidget, isGridOverlay } from "./desktop";

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
export type SearchEngine = "google" | "yandex" | "duckduckgo";

/** A group of pinned apps, shown as one tile. */
export interface PinFolder {
  name: string;
  items: string[];
}

/** Pinned ids of folders: `folder:<key>`, the key into `folders`. */
export const FOLDER_PREFIX = "folder:";

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
  agentsChatLimit: number;
  quickLlmModel: string;
  /** The same for the notes list next to an open note. */
  notesListWidth: number | null;
  /** "Search the web" row's engine. */
  searchEngine: SearchEngine;
  /** Apps left out of "all apps" and search. */
  hiddenApps: string[];
  /** The user's own names for apps, by id. */
  appNames: Record<string, string>;
  /** Folders among the pinned, by key (see FOLDER_PREFIX). */
  folders: Record<string, PinFolder>;
  /** Collapsed state of collapsible blocks (widgets, `settings.*` groups), by id; each picks its own default. */
  collapsedWidgets: Record<string, boolean>;
  /** Widgets on the desktop: background opacity, 0–100, over the blur if on. */
  deskOpacity: number;
  deskBlur: boolean;
  /** Width of a widget, logical px, in the panel's columns and on the desktop. */
  widgetWidth: number;
  togglePin: (id: string) => void;
  /** The pinned in a new order (folders included). */
  setPinned: (ids: string[]) => void;
  /** Puts `id` into the folder `target` is, or makes a folder of the two. */
  groupPinned: (id: string, target: string) => void;
  renameFolder: (key: string, name: string) => void;
  /** Takes an app out of its folder, back next to the folder (the folder goes when one app is left). */
  ungroup: (id: string) => void;
  /** All apps of the folder back among the pinned. */
  dissolveFolder: (key: string) => void;
  setSearchEngine: (engine: SearchEngine) => void;
  setAppHidden: (id: string, hidden: boolean) => void;
  /** An empty name goes back to the app's own. */
  renameApp: (id: string, name: string) => void;
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
  setAgentsChatLimit: (limit: number) => void;
  setQuickLlmModel: (model: string) => void;
  setNotesListWidth: (width: number) => void;
  setWidgetCollapsed: (id: string, collapsed: boolean) => void;
  setDeskOpacity: (opacity: number) => void;
  setDeskBlur: (on: boolean) => void;
  setWidgetWidth: (width: number) => void;
}

/** The folder an app is in, if any. */
export function folderOf(folders: Record<string, PinFolder>, id: string) {
  return Object.keys(folders).find((k) => folders[k].items.includes(id)) ?? null;
}

/** A folder with one app left becomes that app; an empty one goes. */
function tidy(pinned: string[], folders: Record<string, PinFolder>) {
  const next = { ...folders };
  let order = pinned;
  for (const [key, f] of Object.entries(next)) {
    if (f.items.length > 1) continue;
    order = order.flatMap((p) => (p === FOLDER_PREFIX + key ? f.items : [p]));
    delete next[key];
  }
  return { pinned: order, folders: next };
}

/** Drops an app that `ungroup` just put back among the pinned. */
function unpinLoose(id: string) {
  const pinned = usePrefs.getState().pinned.filter((p) => p !== id);
  usePrefs.setState({ pinned });
  persist("pinned", pinned);
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
  agentsChatLimit: 0,
  quickLlmModel: "",
  notesListWidth: null,
  collapsedWidgets: {},
  deskOpacity: 78,
  deskBlur: true,
  widgetWidth: WIDGET_DEFAULT,
  searchEngine: "google",
  hiddenApps: [],
  appNames: {},
  folders: {},
  togglePin: (id) => {
    if (folderOf(get().folders, id)) {
      // Unpinning an app in a folder takes it out of both.
      get().ungroup(id);
      unpinLoose(id);
      return;
    }
    const current = get().pinned;
    const pinned = current.includes(id) ? current.filter((p) => p !== id) : [...current, id];
    set({ pinned });
    persist("pinned", pinned);
  },
  setPinned: (pinned) => {
    set({ pinned });
    persist("pinned", pinned);
  },
  groupPinned: (id, target) => {
    if (id === target) return;
    const { pinned, folders } = get();
    let next = { ...folders };
    let order = pinned.filter((p) => p !== id);
    // Out of the folder it was in, if any.
    for (const [key, f] of Object.entries(next)) if (f.items.includes(id)) next[key] = { ...f, items: f.items.filter((i) => i !== id) };
    if (target.startsWith(FOLDER_PREFIX)) {
      const key = target.slice(FOLDER_PREFIX.length);
      next[key] = { ...next[key], items: [...next[key].items, id] };
    } else {
      const key = Date.now().toString(36);
      next[key] = { name: "Папка", items: [target, id] };
      order = order.map((p) => (p === target ? FOLDER_PREFIX + key : p));
    }
    ({ pinned: order, folders: next } = tidy(order, next));
    set({ pinned: order, folders: next });
    persist("pinned", order);
    persist("folders", next);
  },
  renameFolder: (key, name) => {
    const folders = { ...get().folders, [key]: { ...get().folders[key], name: name.trim() || "Папка" } };
    set({ folders });
    persist("folders", folders);
  },
  ungroup: (id) => {
    const { pinned, folders } = get();
    const key = folderOf(folders, id);
    if (!key) return;
    const next = { ...folders, [key]: { ...folders[key], items: folders[key].items.filter((i) => i !== id) } };
    const at = pinned.indexOf(FOLDER_PREFIX + key);
    const order = [...pinned.slice(0, at + 1), id, ...pinned.slice(at + 1)];
    const tidied = tidy(order, next);
    set(tidied);
    persist("pinned", tidied.pinned);
    persist("folders", tidied.folders);
  },
  dissolveFolder: (key) => {
    const { pinned, folders } = get();
    const items = folders[key]?.items ?? [];
    const order = pinned.flatMap((p) => (p === FOLDER_PREFIX + key ? items : [p]));
    const next = { ...folders };
    delete next[key];
    set({ pinned: order, folders: next });
    persist("pinned", order);
    persist("folders", next);
  },
  setSearchEngine: (searchEngine) => {
    set({ searchEngine });
    persist("searchEngine", searchEngine);
  },
  setAppHidden: (id, hidden) => {
    const current = get().hiddenApps.filter((h) => h !== id);
    const hiddenApps = hidden ? [...current, id] : current;
    set({ hiddenApps });
    persist("hiddenApps", hiddenApps);
  },
  renameApp: (id, name) => {
    const appNames = { ...get().appNames };
    if (name.trim()) appNames[id] = name.trim();
    else delete appNames[id];
    set({ appNames });
    persist("appNames", appNames);
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
  setQuickLlmModel: (quickLlmModel) => {
    set({ quickLlmModel });
    persist("quickLlmModel", quickLlmModel);
  },
  setAgentsChatLimit: (limit) => {
    const agentsChatLimit = [0, 3, 5, 10, 20].includes(limit) ? limit : 0;
    set({ agentsChatLimit });
    persist("agentsChatLimit", agentsChatLimit);
  },
  setMailListWidth: (mailListWidth) => {
    set({ mailListWidth });
    persist("mailListWidth", mailListWidth);
  },
  setNotesListWidth: (notesListWidth) => {
    set({ notesListWidth });
    persist("notesListWidth", notesListWidth);
  },
  setWidgetCollapsed: (id, collapsed) => {
    const collapsedWidgets = { ...get().collapsedWidgets, [id]: collapsed };
    set({ collapsedWidgets });
    persist("collapsedWidgets", collapsedWidgets);
  },
  setDeskOpacity: (deskOpacity) => {
    set({ deskOpacity });
    persist("deskOpacity", deskOpacity);
  },
  setDeskBlur: (deskBlur) => {
    set({ deskBlur });
    persist("deskBlur", deskBlur);
  },
  setWidgetWidth: (widgetWidth) => {
    set({ widgetWidth });
    persist("widgetWidth", widgetWidth);
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
      quickLlmModel: (await s.get<string>("quickLlmModel")) ?? "",
      agentsChatLimit: (await s.get<number>("agentsChatLimit")) ?? 0,
      mailListWidth: (await s.get<number>("mailListWidth")) ?? null,
      notesListWidth: (await s.get<number>("notesListWidth")) ?? null,
      collapsedWidgets: (await s.get<Record<string, boolean>>("collapsedWidgets")) ?? {},
      searchEngine: (await s.get<SearchEngine>("searchEngine")) ?? "google",
      hiddenApps: (await s.get<string[]>("hiddenApps")) ?? [],
      appNames: (await s.get<Record<string, string>>("appNames")) ?? {},
      folders: (await s.get<Record<string, PinFolder>>("folders")) ?? {},
      deskOpacity: (await s.get<number>("deskOpacity")) ?? 78,
      deskBlur: (await s.get<boolean>("deskBlur")) ?? true,
      widgetWidth: (await s.get<number>("widgetWidth")) ?? WIDGET_DEFAULT,
    });
    // Widgets were 340 px wide before the setting: keep the panel's number of
    // columns with the new default width. Once, from the panel window.
    if (!(await s.has("widgetWidth")) && !deskWidget && !isGridOverlay) {
      persist("widgetWidth", WIDGET_DEFAULT);
      const { width } = await invoke<{ width: number }>("panel_settings");
      await invoke("panel_set_width", { width: panelWidthFor(columnsIn(width, 340), WIDGET_DEFAULT), persist: true });
      window.dispatchEvent(new Event(PANEL_SETTINGS_CHANGED));
    }
    // The panel and the widgets on the desktop are separate windows sharing
    // this store: take in what the others change.
    await s.onChange((key, value) => {
      if (value !== undefined && key in usePrefs.getState()) usePrefs.setState({ [key]: value } as Partial<PrefsState>);
    });
  })
  .catch((e) => console.error("prefs load failed", e));
