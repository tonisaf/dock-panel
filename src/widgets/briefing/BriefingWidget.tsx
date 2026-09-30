import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, RefreshCw, Sunrise } from "lucide-react";
import { Card } from "../../components/Card";
import { useMailList, useMailSettings, useUnread } from "../../mail/api";
import { useUpcoming } from "../calendar/api";
import { useTasks } from "../tasks/api";
import { useWeather } from "../weather/api";
import { buildBriefingContext } from "./context";

const KEY = "briefing.v1";

interface Saved {
  day: string;
  at: number;
  text: string;
}

function loadSaved(): Saved | null {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as Saved | null;
    return saved?.day === new Date().toDateString() ? saved : null;
  } catch {
    return null;
  }
}

/** The day in a few sentences from the local LM Studio model; made on request, kept for the day. */
export function BriefingWidget() {
  const [saved, setSaved] = useState<Saved | null>(loadSaved);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const weather = useWeather().data;
  const { occurrences } = useUpcoming(2);
  const tasks = useTasks().data?.tasks ?? [];
  const { data: mail } = useMailSettings();
  const { messages } = useMailList(null, true, (mail?.accounts.length ?? 0) > 0);
  const unread = useUnread().data?.total ?? 0;

  const make = async () => {
    setBusy(true);
    setError(null);
    try {
      const now = new Date();
      const text = await invoke<string>("llm_run", {
        task: "briefing",
        text: buildBriefingContext({
          now,
          weather,
          events: occurrences,
          tasks,
          unread,
          unreadLetters: messages.filter((m) => m.unread),
        }),
      });
      const next = { day: now.toDateString(), at: now.getTime(), text };
      try {
        localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        // Storage can be unavailable; the briefing still shows.
      }
      setSaved(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const button = (
    <button
      onClick={make}
      disabled={busy}
      title="Составить заново"
      className="grid size-7 place-items-center rounded-lg border border-stroke text-fg-subtle hover:bg-ink/8 hover:text-fg disabled:opacity-50"
    >
      {busy ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
    </button>
  );

  return (
    <Card title="Брифинг" icon={Sunrise} action={saved ? button : undefined}>
      {saved ? (
        <>
          <p className="text-[13px] leading-relaxed whitespace-pre-wrap select-text">{saved.text}</p>
          <p className="mt-2 text-[11px] text-fg-subtle">
            Составлен в {new Date(saved.at).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}
          </p>
        </>
      ) : (
        <button
          onClick={make}
          disabled={busy}
          className="flex items-center gap-2 text-left text-[12.5px] text-fg-muted hover:text-fg disabled:opacity-60"
        >
          {busy && <Loader2 className="size-3.5 animate-spin" />}
          {busy ? "LM Studio составляет брифинг…" : "Составить брифинг на сегодня: погода, события, задачи и почта →"}
        </button>
      )}
      {error && <p className="mt-2 text-[12px] leading-relaxed text-warn">{error}</p>}
    </Card>
  );
}
