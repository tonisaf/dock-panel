/** "42 мин", "3 ч 05 мин", "112 ч"; under a minute is "< 1 мин", nothing at all "0 мин". */
export function formatListened(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return ms > 0 ? "< 1 мин" : "0 мин";
  if (minutes < 60) return `${minutes} мин`;
  const hours = Math.floor(minutes / 60);
  if (hours >= 100) return `${hours} ч`;
  return `${hours} ч ${String(minutes % 60).padStart(2, "0")} мин`;
}

/** The hour of day with the most listening; null when there was none. */
export function peakHour(hours: number[]): number | null {
  let best = -1;
  hours.forEach((ms, h) => {
    if (ms > 0 && (best < 0 || ms > hours[best])) best = h;
  });
  return best < 0 ? null : best;
}

const MONTHS = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];

/** "2026-09-30" → "30 сен"; anything else comes back unchanged. */
export function shortDay(day: string): string {
  const m = /^\d{4}-(\d{2})-(\d{2})$/.exec(day);
  const month = m ? MONTHS[Number(m[1]) - 1] : undefined;
  return m && month ? `${Number(m[2])} ${month}` : day;
}
