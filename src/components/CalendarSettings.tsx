import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2, Plus, X } from "lucide-react";
import { useCalendars, type CalendarInfo } from "../widgets/calendar/api";

export function CalendarSettings() {
  const queryClient = useQueryClient();
  const { data: calendars = [] } = useCalendars();
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const add = async () => {
    if (!url.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke<CalendarInfo>("calendar_add", { url });
      setUrl("");
      await queryClient.invalidateQueries({ queryKey: ["calendars"] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string) => {
    await invoke("calendar_remove", { id });
    queryClient.removeQueries({ queryKey: ["calendar-ics", id] });
    await queryClient.invalidateQueries({ queryKey: ["calendars"] });
  };

  return (
    <div className="flex flex-col gap-2 p-3.5">
      {calendars.map((c) => (
        <div key={c.id} className="flex items-center gap-2.5 text-[13.5px]">
          <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: c.color }} />
          <span className="min-w-0 flex-1 truncate">{c.name}</span>
          <button
            onClick={() => remove(c.id)}
            title="Удалить календарь"
            className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}

      <div className="flex gap-2">
        <input
          type="password"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="Секретная ссылка iCal (.ics)"
          spellCheck={false}
          className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 text-[13px] outline-none placeholder:text-fg-subtle focus:border-accent/50"
        />
        <button
          onClick={add}
          disabled={busy || !url.trim()}
          className="flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50"
        >
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Plus className="size-3.5" />} Добавить
        </button>
      </div>
      {error && <p className="text-[12px] text-warn">{error}</p>}

      <details className="text-[12px] leading-relaxed text-fg-subtle">
        <summary className="cursor-pointer select-none hover:text-fg-muted">Где взять ссылку</summary>
        <ul className="mt-1.5 list-disc space-y-1 pl-4">
          <li>
            <b className="font-medium text-fg-muted">Google:</b> calendar.google.com → ⚙ Настройки → слева выберите
            календарь → «Интеграция календаря» → «Секретный адрес в формате iCal».
          </li>
          <li>
            <b className="font-medium text-fg-muted">Outlook:</b> Настройки → Календарь → Общие календари → «Опубликовать
            календарь» → ссылка ICS.
          </li>
          <li>
            <b className="font-medium text-fg-muted">Яндекс:</b> Настройки календаря → «Экспорт» → ссылка iCal.
          </li>
        </ul>
        <p className="mt-1.5">Ссылка даёт доступ к календарю на чтение: панель хранит её локально и не показывает.</p>
      </details>
    </div>
  );
}
