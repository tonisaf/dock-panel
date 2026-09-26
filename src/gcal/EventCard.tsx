import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AlignLeft, CalendarDays, ExternalLink, Loader2, MapPin, Pencil, Repeat, Trash2, Video, X } from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../store";
import {
  addDays,
  bounds,
  hhmm,
  localInput,
  timesBody,
  useGcalActions,
  ymd,
  type Calendar,
  type GEvent,
} from "./api";

/** What the card shows: an existing event, or a new one being drawn. */
export type CardTarget =
  | { kind: "event"; event: GEvent; x: number; y: number }
  | { kind: "new"; start: Date; end: Date; allDay: boolean; x: number; y: number };

const URL_RE = /(https?:\/\/[^\s<>"')\]]+)/g;

function Linkified({ text }: { text: string }) {
  return (
    <>
      {text.split(URL_RE).map((part, i) =>
        i % 2 ? (
          <a
            key={i}
            href={part}
            onClick={(e) => {
              e.preventDefault();
              openUrl(part).catch(console.error);
            }}
            className="break-all text-accent hover:underline"
          >
            {part}
          </a>
        ) : (
          part
        ),
      )}
    </>
  );
}

/** Descriptions from Google can be HTML; the card shows them as text. */
function plain(html: string) {
  const doc = new DOMParser().parseFromString(html.replace(/<br\s*\/?>/gi, "\n").replace(/<\/p>/gi, "\n"), "text/html");
  return (doc.body.textContent ?? "").trim();
}

function when(start: Date, end: Date, allDay: boolean) {
  const day = (d: Date) => d.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" });
  if (allDay) {
    const last = addDays(end, -1);
    return last.getTime() > start.getTime() ? `${day(start)} – ${day(last)} · весь день` : `${day(start)} · весь день`;
  }
  const sameDate = ymd(start) === ymd(end) || (end.getHours() === 0 && end.getMinutes() === 0 && ymd(addDays(start, 1)) === ymd(end));
  return sameDate ? `${day(start)} · ${hhmm(start)}–${hhmm(end)}` : `${day(start)} ${hhmm(start)} – ${day(end)} ${hhmm(end)}`;
}

const field =
  "w-full rounded-lg border border-stroke bg-field px-2.5 py-1.5 text-[13px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50";

export function EventCard({
  target,
  calendars,
  onClose,
}: {
  target: CardTarget;
  calendars: Calendar[];
  onClose: () => void;
}) {
  const { create, update, remove } = useGcalActions();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: target.x, top: target.y });
  const isNew = target.kind === "new";
  const event = target.kind === "event" ? target.event : null;
  const initial = event ? bounds(event) : { start: (target as { start: Date }).start, end: (target as { end: Date }).end };
  const writable = calendars.filter((c) => c.writable);

  const [editing, setEditing] = useState(isNew);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState(event?.title === "(Без названия)" ? "" : (event?.title ?? ""));
  const [allDay, setAllDay] = useState(event?.allDay ?? (target.kind === "new" && target.allDay));
  const [start, setStart] = useState(localInput(initial.start));
  const [end, setEnd] = useState(localInput(allDay ? addDays(initial.end, -1) : initial.end));
  const [location, setLocation] = useState(event?.location ?? "");
  const [description, setDescription] = useState(event?.description ? plain(event.description) : "");
  const [calendarId, setCalendarId] = useState(
    event?.calendarId ?? (writable.find((c) => c.primary) ?? writable[0])?.id ?? "",
  );
  const calendar = calendars.find((c) => c.id === (event?.calendarId ?? calendarId));

  // Keep the card on screen, beside the point it was opened from.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const left = target.x + 12 + width > window.innerWidth - 8 ? target.x - width - 12 : target.x + 12;
    setPos({ left: Math.max(8, left), top: Math.max(8, Math.min(target.y - 20, window.innerHeight - height - 8)) });
  }, [target, editing]);

  // Esc closes the card (not the panel); clicks outside close it too.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || usePanelStore.getState().query) return;
      e.stopImmediatePropagation();
      onClose();
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onDown);
    };
  }, [onClose]);

  const run = async (job: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await job();
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const save = () => {
    const s = new Date(start);
    let e = new Date(end);
    if (allDay) e = addDays(new Date(`${end.slice(0, 10)}T00:00`), 1);
    if (!(e > s)) {
      setError("Окончание должно быть позже начала");
      return;
    }
    const name = title.trim() || "(Без названия)";
    if (isNew) {
      return run(() => create({ calendarId, title: name, start: s, end: e, allDay, location, description }));
    }
    const body = { summary: name, location, description, ...timesBody(allDay ? new Date(`${start.slice(0, 10)}T00:00`) : s, e, allDay) };
    return run(() => update(event!, { title: name, location: location || null, description: description || null, allDay }, body));
  };

  const icon = "size-4 shrink-0 text-fg-subtle";
  const tool = "grid size-8 place-items-center rounded-lg text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-40";
  const color = event?.color ?? calendar?.color ?? "var(--color-accent)";

  return createPortal(
    <motion.div
      ref={ref}
      initial={{ opacity: 0, scale: 0.97 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.12 }}
      style={pos}
      className="fixed z-50 w-[380px] rounded-2xl border border-ink/10 bg-popover p-3 text-fg shadow-2xl shadow-black/40"
    >
      <div className="flex items-center justify-end gap-0.5">
        {event?.editable && !editing && (
          <button className={tool} title="Изменить" onClick={() => setEditing(true)}>
            <Pencil className="size-4" />
          </button>
        )}
        {event?.editable && (
          <button className={tool} title="Удалить" disabled={busy} onClick={() => run(() => remove(event))}>
            <Trash2 className="size-4" />
          </button>
        )}
        {event?.htmlLink && (
          <button className={tool} title="Открыть в Google Календаре" onClick={() => openUrl(event.htmlLink!).catch(console.error)}>
            <ExternalLink className="size-4" />
          </button>
        )}
        <button className={tool} title="Закрыть (Esc)" onClick={onClose}>
          <X className="size-4" />
        </button>
      </div>

      {editing ? (
        <div className="flex flex-col gap-2 px-1 pb-1">
          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !busy && save()}
            placeholder="Название"
            className={clsx(field, "text-[15px] font-medium")}
          />
          <label className="flex items-center gap-2 text-[12.5px] text-fg-muted">
            <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} className="accent-accent" />
            Весь день
          </label>
          <div className="flex items-center gap-2">
            <input
              type={allDay ? "date" : "datetime-local"}
              value={allDay ? start.slice(0, 10) : start}
              onChange={(e) => setStart(allDay ? `${e.target.value}T00:00` : e.target.value)}
              className={field}
            />
            <span className="text-fg-subtle">–</span>
            <input
              type={allDay ? "date" : "datetime-local"}
              value={allDay ? end.slice(0, 10) : end}
              onChange={(e) => setEnd(allDay ? `${e.target.value}T00:00` : e.target.value)}
              className={field}
            />
          </div>
          {isNew && writable.length > 1 && (
            <select value={calendarId} onChange={(e) => setCalendarId(e.target.value)} className={field}>
              {writable.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          )}
          <input value={location} onChange={(e) => setLocation(e.target.value)} placeholder="Место" className={field} />
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Описание"
            rows={3}
            className={clsx(field, "resize-none")}
          />
          {event?.recurring && (
            <p className="flex items-center gap-1.5 text-[11.5px] text-fg-subtle">
              <Repeat className="size-3.5" /> Изменится только это повторение
            </p>
          )}
          {error && <p className="text-[12px] text-warn">{error}</p>}
          <div className="flex justify-end gap-2 pt-1">
            {!isNew && (
              <button className="rounded-lg px-3 py-1.5 text-[12.5px] text-fg-muted hover:bg-ink/10" onClick={() => setEditing(false)}>
                Отмена
              </button>
            )}
            <button
              disabled={busy || (isNew && !calendarId)}
              onClick={save}
              className="flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-1.5 text-[12.5px] font-medium text-on-accent hover:bg-accent/90 disabled:opacity-50"
            >
              {busy && <Loader2 className="size-3.5 animate-spin" />} Сохранить
            </button>
          </div>
        </div>
      ) : (
        event && (
          <div className="flex flex-col gap-2.5 px-1 pb-1">
            <div className="flex gap-3">
              <span className="mt-1.5 size-3.5 shrink-0 rounded" style={{ background: color }} />
              <div className="min-w-0">
                <h3 className="text-[17px] leading-snug font-semibold select-text">{event.title}</h3>
                <p className="mt-0.5 text-[12.5px] text-fg-muted first-letter:uppercase">
                  {when(initial.start, initial.end, event.allDay)}
                </p>
                {event.recurring && (
                  <p className="mt-0.5 flex items-center gap-1 text-[11.5px] text-fg-subtle">
                    <Repeat className="size-3" /> Повторяется
                  </p>
                )}
              </div>
            </div>
            {event.meetLink && (
              <button
                onClick={() => openUrl(event.meetLink!).catch(console.error)}
                className="ml-6.5 flex w-fit items-center gap-2 rounded-lg bg-accent px-3 py-1.5 text-[12.5px] font-medium text-on-accent hover:bg-accent/90"
              >
                <Video className="size-4" /> Присоединиться
              </button>
            )}
            {event.location && (
              <p className="flex gap-3 text-[12.5px] select-text">
                <MapPin className={icon} /> <Linkified text={event.location} />
              </p>
            )}
            {event.description && (
              <p className="flex gap-3 text-[12.5px] leading-relaxed">
                <AlignLeft className={icon} />
                <span className="max-h-48 min-w-0 overflow-y-auto break-words whitespace-pre-wrap select-text">
                  <Linkified text={plain(event.description)} />
                </span>
              </p>
            )}
            {calendar && (
              <p className="flex items-center gap-3 text-[12.5px] text-fg-muted">
                <CalendarDays className={icon} /> {calendar.name}
              </p>
            )}
            {error && <p className="text-[12px] text-warn">{error}</p>}
          </div>
        )
      )}
    </motion.div>,
    document.body,
  );
}
