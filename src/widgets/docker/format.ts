/** Colour class of the dot in front of a container: running, in between, stopped. */
export function stateTone(state: string): "ok" | "warn" | "off" {
  if (state === "running") return "ok";
  if (state === "paused" || state === "restarting" || state === "created") return "warn";
  return "off";
}

/** "Up 3 hours (healthy)" → "healthy"; null when Docker reports no health check. */
export function health(status: string): "healthy" | "unhealthy" | "starting" | null {
  const m = /\((healthy|unhealthy|health: starting)\)/.exec(status);
  if (!m) return null;
  return m[1] === "health: starting" ? "starting" : (m[1] as "healthy" | "unhealthy");
}

/** Docker's English status in the panel's language: "Up 3 hours" → "3 ч", "Exited (0) 2 days ago" → "2 дн назад". */
export function shortStatus(status: string): string {
  const age = /(\d+|an?|about an?|less than a) (second|minute|hour|day|week|month|year)s?/i.exec(status);
  const unit: Record<string, string> = {
    second: "сек",
    minute: "мин",
    hour: "ч",
    day: "дн",
    week: "нед",
    month: "мес",
    year: "г",
  };
  if (!age) return status;
  const n = /^\d+$/.test(age[1]) ? age[1] : /^less/i.test(age[1]) ? "<1" : "1";
  const text = `${n} ${unit[age[2].toLowerCase()]}`;
  return status.startsWith("Up") ? text : `${text} назад`;
}

/** "3 из 7" for the card header. */
export function summary(running: number, total: number): string {
  return `${running} из ${total}`;
}
