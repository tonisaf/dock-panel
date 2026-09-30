import { describe, expect, it } from "vitest";
import type { Occurrence } from "../calendar/api";
import type { Task } from "../tasks/api";
import type { Summary } from "../../mail/api";
import { buildBriefingContext } from "./context";

const now = new Date(2026, 8, 30, 9, 0);
const event = (title: string, start: Date, end: Date, allDay = false): Occurrence => ({
  key: title,
  calendar: { id: "c", name: "c", color: "#fff" },
  title,
  start,
  end,
  allDay,
  location: null,
  link: null,
});
const task = (title: string, extra: Partial<Task> = {}): Task => ({
  id: title,
  url: "",
  title,
  status: null,
  inProgress: false,
  due: null,
  priority: null,
  tag: null,
  ...extra,
});
const letter = (subject: string): Summary => ({
  account: "a",
  uid: 1,
  fromName: "Анна",
  fromEmail: "anna@example.com",
  subject,
  date: 0,
  unread: true,
  flagged: false,
});

describe("buildBriefingContext", () => {
  it("leaves out sections with nothing in them", () => {
    const text = buildBriefingContext({ now, events: [], tasks: [], unread: 0, unreadLetters: [] });
    expect(text).not.toMatch(/События|Задачи|писем|Погода/);
  });

  it("lists upcoming events with their day and skips finished ones", () => {
    const text = buildBriefingContext({
      now,
      events: [
        event("Планёрка", new Date(2026, 8, 30, 8, 0), new Date(2026, 8, 30, 8, 30)),
        event("Созвон", new Date(2026, 8, 30, 15, 0), new Date(2026, 8, 30, 16, 0)),
        event("Отпуск", new Date(2026, 9, 1, 0, 0), new Date(2026, 9, 2, 0, 0), true),
      ],
      tasks: [],
      unread: 0,
      unreadLetters: [],
    });
    expect(text).not.toContain("Планёрка");
    expect(text).toContain("сегодня 15:00–16:00: Созвон");
    expect(text).toContain("завтра, весь день: Отпуск");
  });

  it("describes tasks and unread letters", () => {
    const text = buildBriefingContext({
      now,
      events: [],
      tasks: [task("Отчёт", { due: "2026-09-30", inProgress: true })],
      unread: 12,
      unreadLetters: [letter("Счёт")],
    });
    expect(text).toContain("- Отчёт, в работе, срок 2026-09-30");
    expect(text).toContain("Непрочитанных писем: 12");
    expect(text).toContain("- Анна: Счёт");
  });
});
