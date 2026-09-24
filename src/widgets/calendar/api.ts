import { useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueries, useQuery } from "@tanstack/react-query";
import ICAL from "ical.js";

export interface CalendarInfo {
  id: string;
  name: string;
  color: string;
}

export interface Occurrence {
  key: string;
  calendar: CalendarInfo;
  title: string;
  start: Date;
  end: Date;
  allDay: boolean;
  location: string | null;
  /** Video meeting link found in the event, if any. */
  link: string | null;
}

export function useCalendars() {
  return useQuery({
    queryKey: ["calendars"],
    queryFn: () => invoke<CalendarInfo[]>("calendar_list"),
    staleTime: Infinity,
  });
}

const MEETING_LINK =
  /https:\/\/(?:meet\.google\.com|[\w.-]*zoom\.us|teams\.microsoft\.com|teams\.live\.com|telemost\.yandex\.ru)[^\s"'<>)\\]*/i;

function findLink(component: ICAL.Component): string | null {
  for (const prop of ["x-google-conference", "url", "location", "description"]) {
    const value = component.getFirstPropertyValue(prop);
    const match = typeof value === "string" ? value.match(MEETING_LINK) : null;
    if (match) return match[0];
  }
  return null;
}

const cancelled = (c: ICAL.Component) => String(c.getFirstPropertyValue("status") ?? "").toUpperCase() === "CANCELLED";

/** Occurrences of every event (recurring ones expanded) overlapping [from, to). */
export function expandCalendar(ics: string, calendar: CalendarInfo, from: Date, to: Date): Occurrence[] {
  const root = new ICAL.Component(ICAL.parse(ics));
  for (const tz of root.getAllSubcomponents("vtimezone")) {
    ICAL.TimezoneService.register(tz);
  }

  // Moved or edited instances of a series arrive as separate VEVENTs with a RECURRENCE-ID.
  const masters = new Map<string, ICAL.Event>();
  const exceptions: ICAL.Event[] = [];
  for (const vevent of root.getAllSubcomponents("vevent")) {
    const event = new ICAL.Event(vevent);
    if (event.isRecurrenceException()) exceptions.push(event);
    else masters.set(event.uid, event);
  }
  const orphans: ICAL.Event[] = [];
  for (const ex of exceptions) {
    const master = masters.get(ex.uid);
    if (master) master.relateException(ex);
    else orphans.push(ex);
  }

  const out: Occurrence[] = [];
  const push = (item: ICAL.Event, start: ICAL.Time, end: ICAL.Time) => {
    if (cancelled(item.component)) return;
    const s = start.toJSDate();
    const e = end.toJSDate();
    if (e <= from || s >= to) return;
    out.push({
      key: `${calendar.id}:${item.uid}:${s.getTime()}`,
      calendar,
      title: item.summary || "Без названия",
      start: s,
      end: e,
      allDay: start.isDate,
      location: item.location || null,
      link: findLink(item.component),
    });
  };

  for (const event of [...masters.values(), ...orphans]) {
    if (cancelled(event.component)) continue;
    if (!event.isRecurring()) {
      push(event, event.startDate, event.endDate ?? event.startDate);
      continue;
    }
    const it = event.iterator();
    // Old daily series can have thousands of past instances; cap the walk.
    for (let i = 0, next = it.next(); next && i < 20_000; i++, next = it.next()) {
      if (next.toJSDate() >= to) break;
      const details = event.getOccurrenceDetails(next);
      push(details.item, details.startDate, details.endDate);
    }
  }
  return out;
}

/** Upcoming occurrences across all calendars for the next `days` days. */
export function useUpcoming(days: number) {
  const { data: calendars = [] } = useCalendars();
  const feeds = useQueries({
    queries: calendars.map((c) => ({
      queryKey: ["calendar-ics", c.id],
      queryFn: () => invoke<string>("calendar_ics", { id: c.id }),
      staleTime: 5 * 60_000,
      refetchInterval: 10 * 60_000,
    })),
  });

  const texts = feeds.map((f) => f.data);
  const dayKey = new Date().toDateString();
  const occurrences = useMemo(() => {
    const from = new Date();
    from.setHours(0, 0, 0, 0);
    const to = new Date(from.getTime() + days * 86_400_000);
    const all: Occurrence[] = [];
    calendars.forEach((c, i) => {
      const ics = texts[i];
      if (!ics) return;
      try {
        all.push(...expandCalendar(ics, c, from, to));
      } catch (e) {
        console.error(`calendar ${c.name} failed to parse`, e);
      }
    });
    return all.sort((a, b) => a.start.getTime() - b.start.getTime());
    // Texts change identity only when refetched; dayKey rolls the window at midnight.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [calendars, days, dayKey, ...texts]);

  return {
    calendars,
    occurrences,
    isLoading: feeds.some((f) => f.isPending),
    error: feeds.find((f) => f.error)?.error ?? null,
  };
}

export function dayLabel(d: Date, now = new Date()) {
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(d) - start(now)) / 86_400_000);
  if (diff === 0) return "Сегодня";
  if (diff === 1) return "Завтра";
  const label = d.toLocaleDateString("ru-RU", { weekday: "short", day: "numeric", month: "short" }).replace(".", "");
  return label.charAt(0).toUpperCase() + label.slice(1);
}

export const hhmm = (d: Date) => d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });

export function relative(o: Occurrence, now: Date) {
  if (o.start <= now && o.end > now) return { text: "идёт сейчас", live: true };
  const min = Math.round((o.start.getTime() - now.getTime()) / 60_000);
  if (min < 60) return { text: `через ${min} мин`, live: false };
  if (min < 12 * 60) return { text: `через ${Math.floor(min / 60)} ч ${min % 60} мин`, live: false };
  return null;
}
