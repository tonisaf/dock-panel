import type { AppEntry } from "./apps";
import type { Usage } from "./prefs";

const EN = "`qwertyuiop[]asdfghjkl;'zxcvbnm,.";
const RU = "ёйцукенгшщзхъфывапролджэячсмитьбю";
const toRu = new Map([...EN].map((c, i) => [c, RU[i]]));
const toEn = new Map([...RU].map((c, i) => [c, EN[i]]));
const wordsOf = (text: string) => text.match(/[\p{L}\p{N}]+/gu) ?? [];

function swapLayout(text: string) {
  const map = [...text].some((c) => toEn.has(c)) ? toEn : toRu;
  return [...text].map((c) => map.get(c) ?? c).join("");
}

function singleScore(name: string, query: string) {
  if (name === query) return 1000;
  if (name.startsWith(query)) return 800;
  const words = wordsOf(name);
  if (words.some((w) => w.startsWith(query))) return 600;
  if (words.map((w) => w[0]).join("").startsWith(query)) return 500;
  const at = name.indexOf(query);
  if (at >= 0) return 400 - Math.min(at, 100);
  if (query.length < 2) return 0;
  let i = 0;
  for (const ch of name) if (ch === query[i]) i++;
  return i === query.length ? 100 : 0;
}

/** Every query word must match a separate name word, in any order. */
function multiScore(name: string, tokens: string[]) {
  const words = wordsOf(name);
  if (tokens.length > words.length) return 0;
  const edges = tokens.map((token) => {
    const alt = swapLayout(token);
    const score = (word: string, q: string) => word === q ? 640 : word.startsWith(q) ? 600 : q.length >= 2 && word.includes(q) ? 350 : 0;
    return words.map((word, index) => ({ index, score: Math.max(score(word, token), alt === token ? 0 : score(word, alt) - 50) }))
      .filter((edge) => edge.score > 0).sort((a, b) => b.score - a.score);
  });
  // Bipartite matching avoids reusing one word for multiple query tokens.
  const owners = new Map<number, number>();
  function assign(token: number, seen: Set<number>): boolean {
    for (const edge of edges[token]) {
      if (seen.has(edge.index)) continue;
      seen.add(edge.index);
      const owner = owners.get(edge.index);
      if (owner === undefined || assign(owner, seen)) { owners.set(edge.index, token); return true; }
    }
    return false;
  }
  if (!tokens.every((_, i) => assign(i, new Set()))) return 0;
  const scores = [...owners].map(([word, token]) => edges[token].find((e) => e.index === word)!.score);
  // The weakest token determines relevance; extra strong matches only break ties.
  return Math.min(...scores) + Math.min(20, scores.reduce((sum, score) => sum + score, 0) / scores.length / 32);
}

function usageBoost(usage: Usage | undefined, now: number) {
  if (!usage || !Number.isFinite(usage.count) || !Number.isFinite(usage.last)) return 0;
  const days = Math.max(0, now - usage.last) / 86_400_000;
  return Math.min(30, Math.log2(1 + Math.max(0, usage.count)) * 6) * 0.5 ** (days / 14)
    + 10 * 0.5 ** (days / 3);
}

export function searchApps(apps: AppEntry[], query: string, usage: Record<string, Usage>, now = Date.now()) {
  const q = query.trim().toLowerCase().replace(/\s+/g, " ");
  if (!q) return [];
  const tokens = wordsOf(q);
  const alt = swapLayout(q);
  return apps.map((app) => {
    const name = app.name.toLowerCase();
    // A multiword query never falls back to scattered letters across the whole name.
    const phrase = tokens.length > 1
      ? name === q ? 1000 : name.startsWith(q) ? 800 : 0
      : singleScore(name, q);
    const alternate = tokens.length > 1
      ? name === alt ? 950 : name.startsWith(alt) ? 750 : 0
      : alt === q ? 0 : singleScore(name, alt) - 50;
    const base = Math.max(phrase, alternate, tokens.length > 1 ? multiScore(name, tokens) : 0);
    return { app, base, score: base + usageBoost(usage[app.id], now) };
  }).filter((s) => s.base > 0)
    .sort((a, b) => b.score - a.score || a.app.name.localeCompare(b.app.name) || a.app.id.localeCompare(b.app.id))
    .slice(0, 50).map((s) => s.app);
}
