import { Fragment, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CalendarDays, MapPin, Video } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { dayLabel, hhmm, relative, useUpcoming, type Occurrence } from "./api";

const DAYS = 7;
const SHOWN = 6;

function useMinute() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(id);
  }, []);
  return now;
}

function EventRow({ o, now }: { o: Occurrence; now: Date }) {
  const rel = o.allDay ? null : relative(o, now);
  const join = (url: string) => {
    openUrl(url).catch(console.error);
    usePanelStore.getState().setOpen(false);
  };

  return (
    <div
      className={clsx(
        "flex items-stretch gap-2.5 rounded-xl px-2 py-1.5",
        rel?.live ? "bg-accent/10" : "hover:bg-surface",
      )}
    >
      <div className="w-1 shrink-0 rounded-full" style={{ backgroundColor: o.calendar.color }} />
      <div className="w-11 shrink-0 text-[12px] leading-tight tabular-nums">
        {o.allDay ? (
          <span className="text-fg-muted">весь день</span>
        ) : (
          <>
            <div>{hhmm(o.start)}</div>
            <div className="text-fg-subtle">{hhmm(o.end)}</div>
          </>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] leading-snug">{o.title}</div>
        <div className="flex items-center gap-2 text-[11.5px] text-fg-subtle">
          {rel && <span className={clsx(rel.live ? "text-accent" : "text-warn/90")}>{rel.text}</span>}
          {o.location && !o.location.startsWith("http") && (
            <span className="flex min-w-0 items-center gap-1 truncate">
              <MapPin className="size-3 shrink-0" /> <span className="truncate">{o.location}</span>
            </span>
          )}
        </div>
      </div>
      {o.link && (
        <button
          onClick={() => join(o.link!)}
          title="Подключиться к встрече"
          className={clsx(
            "grid size-7 shrink-0 place-items-center self-center rounded-lg transition-colors",
            rel?.live ? "bg-accent text-on-accent hover:bg-accent/90" : "text-fg-muted hover:bg-ink/10 hover:text-fg",
          )}
        >
          <Video className="size-4" />
        </button>
      )}
    </div>
  );
}

export function CalendarWidget() {
  const now = useMinute();
  const setTab = usePanelStore((s) => s.setTab);
  const { calendars, occurrences, isLoading, error } = useUpcoming(DAYS);

  if (calendars.length === 0) {
    return (
      <Card title="Календарь" icon={CalendarDays}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Добавьте календарь по iCal-ссылке в настройках →
        </button>
      </Card>
    );
  }

  const upcoming = occurrences.filter((o) => o.end > now).slice(0, SHOWN);
  let lastDay = "";

  return (
    <Card title="Календарь" icon={CalendarDays}>
      {error && upcoming.length === 0 ? (
        <p className="text-[12px] text-warn">{String(error)}</p>
      ) : isLoading && upcoming.length === 0 ? (
        <p className="text-[12px] text-fg-subtle">Загрузка…</p>
      ) : upcoming.length === 0 ? (
        <p className="text-[12px] text-fg-subtle">Ближайшие {DAYS} дней свободны</p>
      ) : (
        <div className="-mx-2 flex flex-col">
          {upcoming.map((o) => {
            const day = dayLabel(o.start < now ? now : o.start, now);
            const header = day !== lastDay;
            lastDay = day;
            return (
              <Fragment key={o.key}>
                {header && <div className="px-2 pt-1.5 pb-0.5 text-[11px] font-medium text-fg-subtle">{day}</div>}
                <EventRow o={o} now={now} />
              </Fragment>
            );
          })}
        </div>
      )}
    </Card>
  );
}
