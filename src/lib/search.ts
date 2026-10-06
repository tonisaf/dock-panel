import { useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { usePanelStore } from "../store";
import { useNoteSearch, type NoteHit } from "../notes/api";
import { FILE_PREFIX, launchApp, useSearchResults, type AppEntry } from "./apps";
import { answer, convert, parseCurrency, type Answer, type Rates } from "./quick";
import { usePrefs, type SearchEngine } from "./prefs";

export interface FileHit {
  name: string;
  path: string;
  /** Containing folder, "~" for the home folder. */
  folder: string;
  dir: boolean;
}

/** One row of the search results, in the order they show and ↑/↓ walk them. */
export type SearchItem =
  | { kind: "answer"; answer: Answer }
  | { kind: "app"; app: AppEntry }
  | { kind: "note"; note: NoteHit }
  | { kind: "file"; file: FileHit; app: AppEntry }
  | { kind: "web"; query: string; engine: SearchEngine; ai?: boolean };

export const ENGINES: Record<SearchEngine, { label: string; url: (q: string) => string }> = {
  google: { label: "Google", url: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}` },
  yandex: { label: "Яндекс", url: (q) => `https://yandex.ru/search/?text=${encodeURIComponent(q)}` },
  duckduckgo: { label: "DuckDuckGo", url: (q) => `https://duckduckgo.com/?q=${encodeURIComponent(q)}` },
};

const FILES_SHOWN = 8;

function useRates(enabled: boolean) {
  return useQuery({
    queryKey: ["currency-rates"],
    queryFn: () => invoke<Rates>("currency_rates"),
    enabled,
    staleTime: 60 * 60_000,
  });
}

function useFileSearch(query: string) {
  const term = query.trim();
  return useQuery({
    queryKey: ["files-search", term],
    queryFn: () => invoke<FileHit[]>("files_search", { query: term, limit: FILES_SHOWN }),
    enabled: term.length >= 2,
    staleTime: 30_000,
    placeholderData: keepPreviousData,
  });
}

/** Everything the search bar finds for the current query. */
export function useSearchItems(): SearchItem[] {
  const query = usePanelStore((s) => s.query);
  const engine = usePrefs((s) => s.searchEngine);
  const apps = useSearchResults();
  const notes = useNoteSearch(query).data ?? [];
  const files = useFileSearch(query).data ?? [];
  const currency = useMemo(() => parseCurrency(query), [query]);
  const rates = useRates(!!currency).data;

  return useMemo(() => {
    const q = query.trim();
    if (!q) return [];
    const quick = answer(q) ?? (currency && rates ? convert(currency, rates) : null);
    // A file already listed as a pinned app shows once.
    const shownFiles = new Set(apps.map((a) => a.id));
    return [
      ...(quick ? [{ kind: "answer" as const, answer: quick }] : []),
      ...apps.map((app) => ({ kind: "app" as const, app })),
      ...notes.map((note) => ({ kind: "note" as const, note })),
      ...files
        .map((file) => ({ kind: "file" as const, file, app: { id: FILE_PREFIX + file.path, name: file.name } }))
        .filter((f) => !shownFiles.has(f.app.id)),
      { kind: "web" as const, query: q, engine },
      { kind: "web" as const, query: q, engine: "google" as const, ai: true },
    ];
  }, [query, apps, notes, files, currency, rates, engine]);
}

/** Enter on a row. `admin` (Ctrl+Shift+Enter) runs an app elevated. */
export async function activateItem(item: SearchItem, { admin = false } = {}) {
  const panel = usePanelStore.getState();
  switch (item.kind) {
    case "answer":
      await navigator.clipboard.writeText(item.answer.copy).catch(console.error);
      panel.setOpen(false);
      return;
    case "app":
    case "file":
      if (admin) {
        usePrefs.getState().recordLaunch(item.app.id);
        await invoke("launch_app_admin", { id: item.app.id }).catch(console.error);
        panel.setOpen(false);
      } else {
        await launchApp(item.app.id);
      }
      return;
    case "note":
      panel.openNote(item.note.id);
      return;
    case "web":
      if (item.ai) {
        usePanelStore.setState((s) => ({ googleQuery: item.query, googleActive: true, googleRequest: s.googleRequest + 1 }));
        return;
      }
      await openUrl(ENGINES[item.engine].url(item.query)).catch(console.error);
      panel.setOpen(false);
  }
}
