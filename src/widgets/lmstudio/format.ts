/** 22_069_769_172 → "20.6 ГБ"; small ones in МБ. */
export function formatSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} ГБ`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} МБ`;
}

/** 65536 → "64k", 262144 → "256k". */
export function formatContext(tokens: number): string {
  return tokens >= 1024 ? `${Math.round(tokens / 1024)}k` : String(tokens);
}

/** How long until a model with `ttlMs` idle time unloads, or null when it never does. */
export function unloadsIn(ttlMs: number | null, lastUsedMs: number | null, now: number): string | null {
  if (!ttlMs || !lastUsedMs) return null;
  const left = Math.max(0, lastUsedMs + ttlMs - now);
  if (left < 60_000) return "меньше минуты";
  const minutes = Math.ceil(left / 60_000);
  if (minutes < 60) return `${minutes} мин`;
  return `${Math.floor(minutes / 60)} ч ${minutes % 60} мин`;
}
