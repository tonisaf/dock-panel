import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown, ExternalLink, FileUp, Loader2, X } from "lucide-react";
import clsx from "clsx";
import { Toggle } from "./Toggle";
import { useYoutubeActions, useYoutubeSettings } from "../widgets/youtube/api";

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";
/** Longer channel lists start collapsed. */
const LIST_COLLAPSED = 6;

export function YoutubeSettings() {
  const { data } = useYoutubeSettings();
  const { add, importTakeout, remove, setOptions } = useYoutubeActions();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState<"add" | "import" | null>(null);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const [showAll, setShowAll] = useState(false);
  const channels = data?.channels ?? [];

  const run = async (kind: "add" | "import", job: () => Promise<string>) => {
    setBusy(kind);
    setNote(null);
    try {
      setNote({ text: await job(), error: false });
    } catch (e) {
      setNote({ text: String(e), error: true });
    } finally {
      setBusy(null);
    }
  };

  const addChannel = () =>
    run("add", async () => {
      const channel = await add(input.trim());
      setInput("");
      return `Добавлен канал «${channel.title}»`;
    });

  const importFile = () =>
    run("import", async () => {
      const count = await importTakeout();
      return count > 0 ? `Добавлено каналов: ${count}` : "Новых каналов в файле нет";
    });

  const shown = showAll ? channels : channels.slice(0, LIST_COLLAPSED);

  return (
    <div className="flex flex-col divide-y divide-stroke">
      <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
        <p>
          Новые видео с каналов по их RSS-лентам: без ключей и входа в Google. Вставьте ссылку на канал, @имя или ссылку
          на любое его видео.
        </p>
        <div className="flex gap-2">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && input.trim() && addChannel()}
            placeholder="youtube.com/@канал"
            spellCheck={false}
            className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 text-[12.5px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50"
          />
          <button className={button} disabled={!!busy || !input.trim()} onClick={addChannel}>
            {busy === "add" && <Loader2 className="size-3.5 animate-spin" />} Добавить
          </button>
        </div>
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <button className={button} disabled={!!busy} onClick={importFile}>
            {busy === "import" ? <Loader2 className="size-3.5 animate-spin" /> : <FileUp className="size-3.5" />}
            Импорт подписок…
          </button>
          <span className="text-[11.5px] text-fg-subtle">
            файл subscriptions.csv из{" "}
            <button
              className="inline-flex items-center gap-0.5 text-accent hover:underline"
              onClick={() => openUrl("https://takeout.google.com/settings/takeout/custom/youtube").catch(console.error)}
            >
              Google Takeout <ExternalLink className="size-3" />
            </button>{" "}
            (YouTube → только «подписки»)
          </span>
        </div>
        {note && <p className={note.error ? "text-warn" : "text-ok"}>{note.text}</p>}
      </div>

      {channels.length > 0 && (
        <div className="flex flex-col px-2 py-1.5">
          <div className="px-1.5 pt-1 pb-0.5 text-[11.5px] text-fg-subtle">Каналы · {channels.length}</div>
          {shown.map((c) => (
            <div key={c.id} className="group flex items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-ink/5">
              <button
                onClick={() => openUrl(`https://www.youtube.com/channel/${c.id}`).catch(console.error)}
                className="min-w-0 flex-1 truncate text-left text-[13px] hover:underline"
              >
                {c.title}
              </button>
              <button
                onClick={() => remove(c.id).catch(console.error)}
                title="Убрать канал"
                className="grid size-6 place-items-center rounded-md text-fg-subtle opacity-0 group-hover:opacity-100 hover:bg-ink/10 hover:text-fg"
              >
                <X className="size-3.5" />
              </button>
            </div>
          ))}
          {channels.length > LIST_COLLAPSED && (
            <button
              onClick={() => setShowAll(!showAll)}
              className="flex items-center gap-1 self-start px-1.5 py-1 text-[12px] text-fg-subtle hover:text-fg"
            >
              <ChevronDown className={clsx("size-3.5 transition-transform", showAll && "rotate-180")} />
              {showAll ? "Свернуть" : `Все ${channels.length}`}
            </button>
          )}
        </div>
      )}

      {data && (
        <>
          <div className="flex items-center justify-between gap-3 px-3.5 py-3">
            <div>
              <div className="text-[14px]">Уведомления о новых видео</div>
              <div className="text-[12px] text-fg-subtle">Ленты обновляются раз в 15 минут</div>
            </div>
            <Toggle on={data.notify} onChange={(notify) => setOptions({ notify }).catch(console.error)} />
          </div>
          <div className="flex items-center justify-between gap-3 px-3.5 py-3">
            <div className="text-[14px]">Скрывать Shorts</div>
            <Toggle on={data.hideShorts} onChange={(hideShorts) => setOptions({ hideShorts }).catch(console.error)} />
          </div>
        </>
      )}
    </div>
  );
}
