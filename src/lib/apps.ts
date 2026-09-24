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

export function useAppsById() {
  const { data } = useApps();
  return useMemo(() => new Map((data ?? []).map((a) => [a.id, a])), [data]);
}

export function useSearchResults() {
  const { data: apps } = useApps();
  const query = usePanelStore((s) => s.query);
  const usage = usePrefs((s) => s.usage);
  return useMemo(() => searchApps(apps ?? [], query, usage), [apps, query, usage]);
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
