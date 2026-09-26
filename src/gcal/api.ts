import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface GcalStatus {
  connected: boolean;
  email: string | null;
  error: string | null;
  /** The sign-in covers YouTube too. */
  youtube: boolean;
}

export interface Calendar {
  id: string;
  name: string;
  color: string;
  primary: boolean;
  writable: boolean;
  visible: boolean;
}

export interface GEvent {
  calendarId: string;
  id: string;
  title: string;
  /** RFC 3339 date-time, or YYYY-MM-DD for all-day events. */
  start: string;
  /** Exclusive; same format as `start`. */
  end: string;
  allDay: boolean;
  color: string;
  location: string | null;
  description: string | null;
  htmlLink: string | null;
  meetLink: string | null;
  recurring: boolean;
  editable: boolean;
}

export interface GTask {
  listId: string;
  list: string;
  id: string;
  title: string;
  /** YYYY-MM-DD. */
  due: string | null;
  /** Due time (RFC 3339) when Google kept a time of day; usually null. */
  dueAt: string | null;
  notes: string | null;
}

export interface TaskList {
  id: string;
  title: string;
}

export type View = "day" | "week" | "month" | "schedule";

// ---- dates -----------------------------------------------------------------------

export const DAY_MS = 86_400_000;

export const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
export const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
export const sameDay = (a: Date, b: Date) => startOfDay(a).getTime() === startOfDay(b).getTime();
/** Monday of the week containing `d`. */
export const startOfWeek = (d: Date) => addDays(startOfDay(d), -((d.getDay() + 6) % 7));

/** ISO 8601 week number. */
export function isoWeek(d: Date) {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  t.setUTCDate(t.getUTCDate() + 4 - (t.getUTCDay() || 7));
  const yearStart = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t.getTime() - yearStart.getTime()) / DAY_MS + 1) / 7);
}

const pad = (n: number) => String(n).padStart(2, "0");
/** "2026-09-26" in local time. */
export const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
/** "2026-09-26T15:00:00+03:00": what Google wants for timed events. */
export function rfc3339(d: Date) {
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  return `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}:00${sign}${pad(Math.floor(Math.abs(off) / 60))}:${pad(Math.abs(off) % 60)}`;
}
/** "2026-09-26T15:00" for <input type="datetime-local">. */
export const localInput = (d: Date) => `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
export const hhmm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** An event's bounds as local dates; all-day events run from midnight to midnight. */
export function bounds(e: GEvent): { start: Date; end: Date } {
  if (e.allDay) {
    const [ys, ms, ds] = e.start.split("-").map(Number);
    const [ye, me, de] = e.end.split("-").map(Number);
    return { start: new Date(ys, ms - 1, ds), end: new Date(ye, me - 1, de) };
  }
  return { start: new Date(e.start), end: new Date(e.end) };
}

/** The days a view shows, and the range to fetch. */
export function viewRange(view: View, anchor: Date): { days: Date[]; from: Date; to: Date } {
  const a = startOfDay(anchor);
  const span = (from: Date, n: number) => ({
    days: Array.from({ length: n }, (_, i) => addDays(from, i)),
    from,
    to: addDays(from, n),
  });
  switch (view) {
    case "day":
      return span(a, 1);
    case "week":
      return span(a, 7);
    case "month":
      return span(startOfWeek(new Date(a.getFullYear(), a.getMonth(), 1)), 42);
    case "schedule":
      return span(a, 30);
  }
}

/** How far ‹ › move the anchor. */
export function step(view: View, anchor: Date, dir: 1 | -1) {
  switch (view) {
    case "day":
      return addDays(anchor, dir);
    case "week":
      return addDays(anchor, 7 * dir);
    case "month":
      return new Date(anchor.getFullYear(), anchor.getMonth() + dir, 1);
    case "schedule":
      return addDays(anchor, 30 * dir);
  }
}

const MONTHS_SHORT = ["янв", "февр", "март", "апр", "май", "июнь", "июль", "авг", "сент", "окт", "нояб", "дек"];
const MONTHS = ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь"];
const cap = (s: string) => s[0].toUpperCase() + s.slice(1);

/** "Сент – окт 2026", "Сентябрь 2026", "Сент 2026 – янв 2027". */
export function rangeTitle(view: View, anchor: Date) {
  if (view === "month") return `${cap(MONTHS[anchor.getMonth()])} ${anchor.getFullYear()}`;
  const { days } = viewRange(view, anchor);
  const first = days[0];
  const last = days[days.length - 1];
  if (first.getMonth() === last.getMonth()) {
    // "27 сентября 2026": with the day, the month takes the genitive.
    return view === "day"
      ? first.toLocaleDateString("ru-RU", { day: "numeric", month: "long", year: "numeric" }).replace(/\s*г\.$/, "")
      : `${cap(MONTHS[first.getMonth()])} ${first.getFullYear()}`;
  }
  if (first.getFullYear() === last.getFullYear()) {
    return `${cap(MONTHS_SHORT[first.getMonth()])} – ${MONTHS_SHORT[last.getMonth()]} ${last.getFullYear()}`;
  }
  return `${cap(MONTHS_SHORT[first.getMonth()])} ${first.getFullYear()} – ${MONTHS_SHORT[last.getMonth()]} ${last.getFullYear()}`;
}

// ---- data ------------------------------------------------------------------------

const STATUS = ["gcal-status"];
const CALENDARS = ["gcal-calendars"];
const EVENTS = ["gcal-events"];
const TASKS = ["gcal-tasks"];

export function useGcalStatus() {
  return useQuery({ queryKey: STATUS, queryFn: () => invoke<GcalStatus>("gcal_status"), staleTime: 5 * 60_000 });
}

export function useCalendars(enabled: boolean) {
  return useQuery({ queryKey: CALENDARS, queryFn: () => invoke<Calendar[]>("gcal_calendars"), enabled, staleTime: 5 * 60_000 });
}

export function useEvents(from: Date, to: Date, enabled: boolean) {
  return useQuery({
    queryKey: [...EVENTS, from.getTime(), to.getTime()],
    queryFn: () => invoke<GEvent[]>("gcal_events", { timeMin: rfc3339(from), timeMax: rfc3339(to) }),
    enabled,
    staleTime: 60_000,
    refetchInterval: 5 * 60_000,
    placeholderData: (prev) => prev,
  });
}

export function useTasks(enabled: boolean) {
  return useQuery({ queryKey: TASKS, queryFn: () => invoke<GTask[]>("gcal_tasks"), enabled, staleTime: 60_000 });
}

/** Only needed while a task is being created. */
export function useTaskLists(enabled: boolean) {
  return useQuery({
    queryKey: ["gcal-task-lists"],
    queryFn: () => invoke<TaskList[]>("gcal_task_lists"),
    enabled,
    staleTime: 10 * 60_000,
  });
}

export interface NewTask {
  listId: string;
  title: string;
  notes: string;
  /** YYYY-MM-DD. */
  due: string | null;
  /** A time of day on `due`, offered to Google (which may drop it). */
  time: string | null;
}

export interface NewEvent {
  calendarId: string;
  title: string;
  start: Date;
  end: Date;
  allDay: boolean;
  description?: string;
  location?: string;
}

/**
 * Google's start/end body for a range. For a PATCH (`patch`), the other kind of
 * time is cleared explicitly: PATCH merges objects, so switching an event to or
 * from "all day" would otherwise leave both `date` and `dateTime` set.
 */
export function timesBody(start: Date, end: Date, allDay: boolean, patch = false) {
  const clear = patch ? (allDay ? { dateTime: null, timeZone: null } : { date: null }) : {};
  return allDay
    ? { start: { date: ymd(start), ...clear }, end: { date: ymd(end), ...clear } }
    : { start: { dateTime: rfc3339(start), ...clear }, end: { dateTime: rfc3339(end), ...clear } };
}

export function useGcalActions() {
  const queryClient = useQueryClient();
  // Every cached event range, patched in place; `undo` puts them back.
  const patchEvents = (fn: (list: GEvent[]) => GEvent[]) => {
    const before = queryClient.getQueriesData<GEvent[]>({ queryKey: EVENTS });
    queryClient.setQueriesData<GEvent[]>({ queryKey: EVENTS }, (l) => l && fn(l));
    return () => before.forEach(([key, data]) => queryClient.setQueryData(key, data));
  };
  const refresh = () => queryClient.invalidateQueries({ queryKey: EVENTS });
  const same = (a: GEvent, b: GEvent) => a.calendarId === b.calendarId && a.id === b.id;
  const update = async (e: GEvent, changes: Partial<GEvent>, body: Record<string, unknown>) => {
    const undo = patchEvents((l) => l.map((x) => (same(x, e) ? { ...x, ...changes } : x)));
    try {
      const saved = await invoke<GEvent>("gcal_update", { calendarId: e.calendarId, eventId: e.id, patch: body });
      patchEvents((l) => l.map((x) => (same(x, e) ? saved : x)));
    } catch (err) {
      undo();
      throw err;
    }
  };

  return {
    refresh: () => Promise.all([refresh(), queryClient.invalidateQueries({ queryKey: TASKS })]),

    create: async (e: NewEvent) => {
      const body = {
        summary: e.title,
        description: e.description || undefined,
        location: e.location || undefined,
        ...timesBody(e.start, e.end, e.allDay),
      };
      const created = await invoke<GEvent>("gcal_create", { calendarId: e.calendarId, event: body });
      patchEvents((l) => [...l, created]);
      refresh();
      return created;
    },

    /** Optimistic: the event changes at once and comes back if Google refuses. */
    update,

    /** Moves or resizes a timed event. */
    retime: (e: GEvent, start: Date, end: Date) =>
      update(e, { start: rfc3339(start), end: rfc3339(end) }, timesBody(start, end, false, true)),

    remove: async (e: GEvent) => {
      const undo = patchEvents((l) => l.filter((x) => !same(x, e)));
      try {
        await invoke("gcal_delete", { calendarId: e.calendarId, eventId: e.id });
      } catch (err) {
        undo();
        throw err;
      }
    },

    createTask: async (t: NewTask) => {
      const created = await invoke<GTask>("gcal_task_create", {
        listId: t.listId,
        title: t.title,
        notes: t.notes || null,
        due: t.due,
        dueAt: t.due && t.time ? rfc3339(new Date(`${t.due}T${t.time}`)) : null,
      });
      queryClient.setQueryData<GTask[]>(TASKS, (l) => l && [...l, created]);
      queryClient.invalidateQueries({ queryKey: TASKS });
      return created;
    },

    completeTask: async (t: GTask) => {
      const before = queryClient.getQueryData<GTask[]>(TASKS);
      queryClient.setQueryData<GTask[]>(TASKS, (l) => l?.filter((x) => x.id !== t.id));
      try {
        await invoke("gcal_task_done", { listId: t.listId, taskId: t.id, done: true });
      } catch (err) {
        queryClient.setQueryData(TASKS, before);
        throw err;
      }
    },

    setVisible: async (id: string, visible: boolean) => {
      queryClient.setQueryData<Calendar[]>(CALENDARS, (l) => l?.map((c) => (c.id === id ? { ...c, visible } : c)));
      await invoke("gcal_set_visible", { id, visible });
      refresh();
    },

    login: async (clientId: string, clientSecret: string) => {
      await invoke<string>("gcal_login", { clientId, clientSecret });
      await queryClient.invalidateQueries({ queryKey: STATUS });
      queryClient.invalidateQueries({ queryKey: CALENDARS });
      refresh();
    },

    logout: async () => {
      await invoke("gcal_logout");
      queryClient.removeQueries({ queryKey: EVENTS });
      queryClient.removeQueries({ queryKey: CALENDARS });
      queryClient.removeQueries({ queryKey: TASKS });
      await queryClient.invalidateQueries({ queryKey: STATUS });
    },
  };
}

/** Text colour that reads on an event's background. */
export function onColor(hex: string) {
  const m = /^#?([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(hex);
  if (!m) return "#fff";
  const [r, g, b] = m.slice(1).map((h) => parseInt(h, 16) / 255);
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return lum > 0.6 ? "#1f1f1f" : "#fff";
}
