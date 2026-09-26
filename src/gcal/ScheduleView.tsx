import { useMemo } from "react";
import clsx from "clsx";
import { addDays, bounds, hhmm, sameDay, type GEvent } from "./api";
import type { CardTarget } from "./EventCard";

/** Days with events, one row per event: time, colour, title, place. */
export function ScheduleView({
  days,
  events,
  onOpen,
  onPickDay,
}: {
  days: Date[];
  events: GEvent[];
  onOpen: (target: CardTarget) => void;
  onPickDay: (day: Date) => void;
}) {
  const now = new Date();
  const groups = useMemo(
    () =>
      days
        .map((d) => {
          const next = addDays(d, 1);
          const list = events
            .filter((e) => {
              const { start, end } = bounds(e);
              return start < next && end > d;
            })
            .sort((a, b) => Number(b.allDay) - Number(a.allDay) || bounds(a).start.getTime() - bounds(b).start.getTime());
          return { day: d, list };
        })
        .filter((g) => g.list.length > 0),
    [days, events],
  );

  if (groups.length === 0) {
    return <p className="px-4 py-8 text-center text-[13px] text-fg-subtle">В эти 30 дней событий нет</p>;
  }

  return (
    <div className="scroll-area h-full">
      <div className="flex flex-col">
        {groups.map(({ day, list }) => {
          const today = sameDay(day, now);
          return (
            <div key={day.getTime()} className="flex gap-4 border-b border-stroke px-2 py-2">
              <button onClick={() => onPickDay(day)} className="flex w-28 shrink-0 items-start gap-2 text-left">
                <span
                  className={clsx(
                    "grid size-8 place-items-center rounded-full text-[16px] tabular-nums",
                    today ? "bg-accent text-on-accent" : "hover:bg-ink/8",
                  )}
                >
                  {day.getDate()}
                </span>
                <span className={clsx("pt-1.5 text-[11.5px] uppercase", today ? "text-accent" : "text-fg-muted")}>
                  {day.toLocaleDateString("ru-RU", { month: "short", weekday: "short" })}
                </span>
              </button>
              <div className="flex min-w-0 flex-1 flex-col">
                {list.map((ev) => {
                  const { start, end } = bounds(ev);
                  return (
                    <button
                      key={`${ev.calendarId}/${ev.id}`}
                      onClick={(e) => onOpen({ kind: "event", event: ev, x: e.clientX, y: e.clientY })}
                      className="flex items-center gap-3 rounded-lg px-2 py-1.5 text-left hover:bg-ink/6"
                    >
                      <span className="size-2.5 shrink-0 rounded-full" style={{ background: ev.color }} />
                      <span className="w-28 shrink-0 text-[12.5px] text-fg-muted tabular-nums">
                        {ev.allDay ? "Весь день" : `${hhmm(start)} – ${hhmm(end)}`}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-[13px]">{ev.title}</span>
                      {ev.location && <span className="max-w-60 truncate text-[12px] text-fg-subtle">{ev.location}</span>}
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
