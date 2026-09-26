import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CalendarDays, ChevronLeft, ChevronRight, Loader2, Plus, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { EmptyState } from "../components/Card";
import { usePanelSettings } from "../lib/panelWidth";
import { usePanelStore } from "../store";
import {
  isoWeek,
  rangeTitle,
  startOfDay,
  step,
  useCalendars,
  useEvents,
  useGcalActions,
  useGcalStatus,
  useTasks,
  viewRange,
  type View,
} from "./api";
import { EventCard, type CardTarget } from "./EventCard";
import { TimeGrid } from "./TimeGrid";
import { MonthView } from "./MonthView";
import { ScheduleView } from "./ScheduleView";

/** The window grows to this width while the calendar is open (as far as the screen allows). */
const CALENDAR_W = 1360;
const VIEWS: { id: View; label: string }[] = [
  { id: "day", label: "День" },
  { id: "week", label: "7 дней" },
  { id: "month", label: "Месяц" },
  { id: "schedule", label: "Расписание" },
];
const VIEW_KEY = "dock-panel:calendar-view";

function savedView(): View {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    if (v && VIEWS.some((x) => x.id === v)) return v as View;
  } catch {
    // Storage may be unavailable; the default is fine.
  }
  return "week";
}

const setExtraWidth = (extra: number, animate: boolean) =>
  invoke<number>("panel_set_extra_width", { extra, animate }).catch(console.error);

/** Grows the window for the calendar, and gives the width back on the way out. */
function useWideWindow() {
  const base = usePanelSettings().width;
  const opened = useRef(false);
  useEffect(() => {
    // Animated on the way in; later width changes (the resize handle) follow at once.
    setExtraWidth(Math.max(0, CALENDAR_W - base), !opened.current);
    opened.current = true;
  }, [base]);
  useEffect(() => () => void setExtraWidth(0, true), []);
}

export function CalendarTab() {
  useWideWindow();
  const setTab = usePanelStore((s) => s.setTab);
  const { data: status, isPending: statusPending } = useGcalStatus();
  const connected = !!status?.connected && !status.error;
  const { data: calendars = [] } = useCalendars(connected);
  const { refresh } = useGcalActions();

  const [view, setViewState] = useState<View>(savedView);
  const [anchor, setAnchor] = useState(() => startOfDay(new Date()));
  const [card, setCard] = useState<CardTarget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const setView = (v: View) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      // Not remembered, that's all.
    }
  };
  const range = useMemo(() => viewRange(view, anchor), [view, anchor]);
  const { data: events = [], isPending, isError, error: loadError, isFetching } = useEvents(range.from, range.to, connected);
  const { data: tasks = [] } = useTasks(connected);

  const go = useCallback((dir: 1 | -1) => setAnchor((a) => step(view, a, dir)), [view]);
  const today = () => setAnchor(view === "month" ? new Date(new Date().getFullYear(), new Date().getMonth(), 1) : startOfDay(new Date()));
  const pickDay = (d: Date) => {
    setAnchor(d);
    setView("day");
  };

  // ← → move through time while nothing else wants the keys.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (card || usePanelStore.getState().query || e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.target instanceof HTMLElement && e.target.closest("input:not([data-panel-search]), textarea, select")) return;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        go(e.key === "ArrowRight" ? 1 : -1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [card, go]);

  if (statusPending) {
    return (
      <p className="flex items-center gap-2 p-4 text-[13px] text-fg-subtle">
        <Loader2 className="size-4 animate-spin" /> Подключаюсь к Google Календарю…
      </p>
    );
  }
  if (!connected) {
    return (
      <div className="flex h-full flex-col">
        <EmptyState
          icon={CalendarDays}
          title={status?.error ? "Google Календарь не отвечает" : "Google Календарь не подключён"}
          text={status?.error ?? "Войдите в Google в настройках панели, и здесь появится ваш календарь."}
        />
        <button onClick={() => setTab("settings")} className="mx-auto -mt-12 text-[12.5px] text-accent hover:underline">
          Открыть настройки
        </button>
      </div>
    );
  }

  const monthOf = view === "month" ? anchor.getMonth() : -1;
  const round = "grid size-9 place-items-center rounded-full text-fg-muted hover:bg-ink/10 hover:text-fg";

  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <header className="flex shrink-0 items-center gap-2 px-1">
        <button
          onClick={today}
          className="rounded-full border border-stroke px-4 py-1.5 text-[13px] font-medium hover:bg-ink/8"
        >
          Сегодня
        </button>
        <button className={round} title="Назад (←)" onClick={() => go(-1)}>
          <ChevronLeft className="size-5" />
        </button>
        <button className={round} title="Вперёд (→)" onClick={() => go(1)}>
          <ChevronRight className="size-5" />
        </button>
        <h2 className="text-[20px] font-normal">{rangeTitle(view, anchor)}</h2>
        {(view === "week" || view === "day") && (
          <span className="rounded bg-ink/10 px-1.5 py-0.5 text-[11px] font-medium text-fg-muted">
            Неделя {isoWeek(range.days[0])}
          </span>
        )}
        <div className="flex-1" />
        <button
          className={round}
          title="Обновить"
          onClick={() => {
            setRefreshing(true);
            refresh().finally(() => setRefreshing(false));
          }}
        >
          <RefreshCw className={clsx("size-4", (refreshing || (isFetching && !isPending)) && "animate-spin")} />
        </button>
        <button
          onClick={(e) => {
            const start = new Date();
            start.setMinutes(0, 0, 0);
            start.setHours(start.getHours() + 1);
            setCard({ kind: "new", start, end: new Date(start.getTime() + 3_600_000), allDay: false, x: e.clientX, y: e.clientY });
          }}
          className="flex items-center gap-1.5 rounded-full bg-accent px-3.5 py-1.5 text-[13px] font-medium text-on-accent hover:bg-accent/90"
        >
          <Plus className="size-4" /> Создать
        </button>
        <div className="flex rounded-full border border-stroke p-0.5">
          {VIEWS.map((v) => (
            <button
              key={v.id}
              onClick={() => setView(v.id)}
              className={clsx(
                "rounded-full px-3 py-1 text-[12.5px] outline-none transition-colors",
                view === v.id ? "bg-accent/20 font-medium text-fg" : "text-fg-muted hover:text-fg",
              )}
            >
              {v.label}
            </button>
          ))}
        </div>
      </header>

      {(error || isError) && (
        <p
          onClick={() => setError(null)}
          className="mx-1 shrink-0 cursor-default rounded-lg border border-warn/30 bg-warn/5 px-2.5 py-1.5 text-[12px] text-warn"
        >
          {error ?? String(loadError)}
        </p>
      )}

      <div className="min-h-0 flex-1 overflow-hidden rounded-2xl border border-stroke bg-surface">
        {isPending ? (
          <p className="flex items-center gap-2 p-4 text-[13px] text-fg-subtle">
            <Loader2 className="size-4 animate-spin" /> Загружаю события…
          </p>
        ) : view === "month" ? (
          <MonthView days={range.days} month={monthOf} events={events} onOpen={setCard} onPickDay={pickDay} />
        ) : view === "schedule" ? (
          <ScheduleView days={range.days} events={events} onOpen={setCard} onPickDay={pickDay} />
        ) : (
          <TimeGrid
            days={range.days}
            events={events}
            tasks={tasks}
            onOpen={setCard}
            onPickDay={pickDay}
            onError={setError}
          />
        )}
      </div>

      {card && <EventCard key={cardKey(card)} target={card} calendars={calendars} onClose={() => setCard(null)} />}
    </div>
  );
}

function cardKey(c: CardTarget) {
  return c.kind === "event" ? `${c.event.calendarId}/${c.event.id}` : `new/${c.start.getTime()}/${c.end.getTime()}`;
}
