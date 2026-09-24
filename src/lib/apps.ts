import { useMemo } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { usePanelStore } from "../store";
import { usePrefs, type Usage } from "./prefs";

export interface AppEntry {
  id: string;
  name: string;
}

export function useApps() {
  return useQuery({
    queryKey: ["apps"],
    queryFn: () => invoke<AppEntry[]>("list_apps"),
    staleTime: 60_000,
  });
}

/** Icons are served lazily by the Rust `appicon` protocol. */
export const iconUrl = (id: string) => convertFileSrc(id, "appicon");

export async function launchApp(id: string) {
  usePrefs.getState().recordLaunch(id);
  try {
    // Launch while we still own the foreground, so the app comes to front.
    await invoke("launch_app", { id });
  } catch (e) {
    console.error(e);
  }
  usePanelStore.getState().setOpen(false);
}

// ---- pinned files ------------------------------------------------------------

/** Pinned files and folders use `file:<absolute path>` as their id. */
export const FILE_PREFIX = "file:";
export const isFileId = (id: string) => id.startsWith(FILE_PREFIX);

/** "C:\Tools\Far.lnk" → "Far"; documents keep their extension. */
export function fileEntry(id: string): AppEntry {
  const path = id.slice(FILE_PREFIX.length).replace(/[\\/]+$/, "");
  const base = path.split(/[\\/]/).pop() || path;
  return { id, name: base.replace(/\.(lnk|exe|url|appref-ms)$/i, "") };
}

/** Opens the system picker and pins what the user chose. */
export async function pinFromDisk(folders: boolean) {
  try {
    const ids = await invoke<string[]>("pick_files", { folders });
    usePrefs.getState().pinMany(ids);
  } catch (e) {
    console.error(e);
  }
}

/** Pinned apps and files in the user's order; apps that were uninstalled drop out. */
export function usePinnedEntries() {
  const byId = useAppsById();
  const pinnedIds = usePrefs((s) => s.pinned);
  return useMemo(
    () =>
      pinnedIds
        .map((id) => (isFileId(id) ? fileEntry(id) : byId.get(id)))
        .filter((a): a is AppEntry => !!a),
    [pinnedIds, byId],
  );
}

export function useAppsById() {
  const { data } = useApps();
  return useMemo(() => new Map((data ?? []).map((a) => [a.id, a])), [data]);
}

export function useSearchResults() {
  const { data: apps } = useApps();
  const pinned = usePinnedEntries();
  const query = usePanelStore((s) => s.query);
  const usage = usePrefs((s) => s.usage);
  // Pinned files are searchable next to the installed apps.
  const all = useMemo(() => [...(apps ?? []), ...pinned.filter((a) => isFileId(a.id))], [apps, pinned]);
  return useMemo(() => searchApps(all, query, usage), [all, query, usage]);
}

// ---- search ------------------------------------------------------------------

const EN = "`qwertyuiop[]asdfghjkl;'zxcvbnm,.";
const RU = "ёйцукенгшщзхъфывапролджэячсмитьбю";
const toRu = new Map([...EN].map((c, i) => [c, RU[i]]));
const toEn = new Map([...RU].map((c, i) => [c, EN[i]]));

/** "ыфкш" → "safari": the query typed in the wrong keyboard layout. */
function swapLayout(q: string) {
  const ru = [...q].some((c) => toEn.has(c));
  const map = ru ? toEn : toRu;
  return [...q].map((c) => map.get(c) ?? c).join("");
}

function matchScore(name: string, q: string) {
  if (name === q) return 1000;
  if (name.startsWith(q)) return 800;
  const words = name.split(/[\s\-_.()]+/).filter(Boolean);
  if (words.some((w) => w.startsWith(q))) return 600;
  if (words.map((w) => w[0]).join("").startsWith(q)) return 500; // "vsc" → Visual Studio Code
  const at = name.indexOf(q);
  if (at >= 0) return 400 - at;
  let i = 0;
  for (const ch of name) if (ch === q[i]) i++;
  return i === q.length ? 100 : 0;
}

export function searchApps(apps: AppEntry[], query: string, usage: Record<string, Usage>) {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const alt = swapLayout(q);

  const scored: { app: AppEntry; score: number }[] = [];
  for (const app of apps) {
    const name = app.name.toLowerCase();
    const base = Math.max(matchScore(name, q), alt === q ? 0 : matchScore(name, alt) - 50);
    if (base > 0) scored.push({ app, score: base + Math.min(usage[app.id]?.count ?? 0, 25) * 4 });
  }
  return scored
    .sort((a, b) => b.score - a.score || a.app.name.localeCompare(b.app.name))
    .slice(0, 50)
    .map((s) => s.app);
}
