import { useMemo } from "react";
import clsx from "clsx";
import { addDays, bounds, hhmm, onColor, sameDay, type GEvent } from "./api";
import type { CardTarget } from "./EventCard";

const WEEKDAYS = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];
const SHOWN = 3;

/** Six weeks from the Monday before the 1st; other months' days are dimmed. */
export function MonthView({
  days,
  month,
  events,
  onOpen,
  onPickDay,
}: {
  days: Date[];
  month: number;
  events: GEvent[];
  onOpen: (target: CardTarget) => void;
  onPickDay: (day: Date) => void;
}) {
  const now = new Date();
  const byDay = useMemo(
    () =>
      days.map((d) => {
        const next = addDays(d, 1);
        return events
          .filter((e) => {
            const { start, end } = bounds(e);
            return start < next && end > d;
          })
          // All-day first, then by time.
          .sort((a, b) => Number(b.allDay) - Number(a.allDay) || bounds(a).start.getTime() - bounds(b).start.getTime());
      }),
    [days, events],
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="grid shrink-0 grid-cols-7 border-b border-stroke">
        {WEEKDAYS.map((w) => (
          <div key={w} className="py-1 text-center text-[11px] font-medium text-fg-muted uppercase">
            {w}
          </div>
        ))}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-7 grid-rows-6">
        {days.map((d, i) => {
          const list = byDay[i];
          const extra = list.length - SHOWN;
          const today = sameDay(d, now);
          return (
            <div
              key={i}
              onDoubleClick={(e) => onOpen({ kind: "new", start: d, end: addDays(d, 1), allDay: true, x: e.clientX, y: e.clientY })}
              className={clsx(
                "flex min-h-0 min-w-0 flex-col gap-0.5 overflow-hidden border-r border-b border-stroke p-1",
                d.getMonth() !== month && "bg-ink/[0.03]",
              )}
            >
              <button
                onClick={() => onPickDay(d)}
                className={clsx(
                  "mx-auto grid size-6 shrink-0 place-items-center rounded-full text-[12px] tabular-nums",
                  today ? "bg-accent font-semibold text-on-accent" : d.getMonth() !== month ? "text-fg-subtle hover:bg-ink/8" : "hover:bg-ink/8",
                )}
              >
                {d.getDate()}
              </button>
              {list.slice(0, SHOWN).map((ev) => (
                <button
                  key={`${ev.calendarId}/${ev.id}`}
                  onClick={(e) => onOpen({ kind: "event", event: ev, x: e.clientX, y: e.clientY })}
                  className={clsx(
                    "flex items-center gap-1 truncate rounded px-1 text-left text-[11.5px] leading-5",
                    !ev.allDay && "hover:bg-ink/8",
                  )}
                  style={ev.allDay ? { background: ev.color, color: onColor(ev.color) } : undefined}
                >
                  {!ev.allDay && <span className="size-2 shrink-0 rounded-full" style={{ background: ev.color }} />}
                  {!ev.allDay && <span className="shrink-0 text-fg-muted tabular-nums">{hhmm(bounds(ev).start)}</span>}
                  <span className="truncate">{ev.title}</span>
                </button>
              ))}
              {extra > 0 && (
                <button onClick={() => onPickDay(d)} className="px-1 text-left text-[11px] text-fg-subtle hover:text-fg">
                  ещё {extra}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
