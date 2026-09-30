import type { Weather } from "../weather/api";
import type { Occurrence } from "../calendar/api";
import type { Task } from "../tasks/api";
import type { Summary } from "../../mail/api";

const MAIL_SHOWN = 8;
const TASKS_SHOWN = 15;

const time = (d: Date) => d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });

export interface BriefingData {
  now: Date;
  weather?: Weather;
  events: Occurrence[];
  tasks: Task[];
  unread: number;
  unreadLetters: Summary[];
}

/** The day's data as plain text for the model; sections with nothing in them are left out. */
export function buildBriefingContext({ now, weather, events, tasks, unread, unreadLetters }: BriefingData): string {
  const sections: string[] = [
    `Сейчас: ${now.toLocaleString("ru-RU", { weekday: "long", day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" })}`,
  ];

  if (weather) {
    const c = weather.current;
    sections.push(
      `Погода: ${Math.round(c.temperature_2m)}°, ощущается как ${Math.round(c.apparent_temperature)}°, ветер ${Math.round(c.wind_speed_10m)} км/ч; ` +
        `сегодня от ${Math.round(weather.daily.temperature_2m_min[0])}° до ${Math.round(weather.daily.temperature_2m_max[0])}°`,
    );
  }

  const today = now.toDateString();
  const rows = events
    .filter((e) => e.end > now)
    .map((e) => {
      const day = e.start.toDateString() === today ? "сегодня" : "завтра";
      const when = e.allDay ? `${day}, весь день` : `${day} ${time(e.start)}–${time(e.end)}`;
      return `- ${when}: ${e.title}${e.location ? ` (${e.location})` : ""}`;
    });
  if (rows.length) sections.push(`События:\n${rows.join("\n")}`);

  const open = tasks.slice(0, TASKS_SHOWN).map((t) => {
    const parts = [t.title];
    if (t.inProgress) parts.push("в работе");
    if (t.due) parts.push(`срок ${t.due}`);
    if (t.priority) parts.push(`приоритет ${t.priority.name}`);
    return `- ${parts.join(", ")}`;
  });
  if (open.length) sections.push(`Задачи:\n${open.join("\n")}`);

  if (unread > 0) {
    const letters = unreadLetters.slice(0, MAIL_SHOWN).map((m) => `- ${m.fromName || m.fromEmail}: ${m.subject}`);
    sections.push(`Непрочитанных писем: ${unread}${letters.length ? `. Последние:\n${letters.join("\n")}` : ""}`);
  }
  return sections.join("\n\n");
}
