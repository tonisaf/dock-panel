import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Database, ExternalLink, Loader2, StickyNote } from "lucide-react";
import clsx from "clsx";
import { usePrefs } from "../lib/prefs";
import { useNotionStatus } from "../widgets/tasks/api";
import { useNotes, useNotesActions } from "../notes/api";

interface Source {
  id: string;
  title: string;
  databaseId: string | null;
}

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

function Connect() {
  const queryClient = useQueryClient();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("notion_set_token", { token });
      setToken("");
      await queryClient.invalidateQueries({ queryKey: ["notion-status"] });
    await queryClient.invalidateQueries({ queryKey: ["notes"] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <ol className="list-decimal space-y-1 pl-4">
        <li>
          Создайте внутреннюю интеграцию на{" "}
          <button
            className="inline-flex items-center gap-0.5 text-accent hover:underline"
            onClick={() => openUrl("https://www.notion.so/profile/integrations").catch(console.error)}
          >
            notion.so/profile/integrations <ExternalLink className="size-3" />
          </button>{" "}
          и скопируйте её секретный токен.
        </li>
        <li>В Notion откройте базу задач → ··· → Connections → добавьте эту интеграцию.</li>
        <li>Вставьте токен сюда. Он хранится в диспетчере учётных данных Windows.</li>
      </ol>
      <div className="flex gap-2">
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && connect()}
          placeholder="ntn_…"
          spellCheck={false}
          className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 text-[13px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50"
        />
        <button className={button} disabled={busy || !token.trim()} onClick={connect}>
          {busy && <Loader2 className="size-3.5 animate-spin" />} Подключить
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}

function SourcePicker() {
  const { notionSource, setNotionSource } = usePrefs();
  const { data, isPending, isError, error, refetch, isFetching } = useQuery({
    queryKey: ["notion-sources"],
    queryFn: () => invoke<Source[]>("notion_list_sources"),
    staleTime: 60_000,
  });

  return (
    <div className="flex flex-col gap-1 p-2">
      <div className="flex items-center justify-between px-1.5 pt-1 pb-1">
        <span className="text-[12px] text-fg-subtle">База задач</span>
        <button onClick={() => refetch()} className="text-[12px] text-fg-subtle hover:text-fg">
          {isFetching ? "Обновляю…" : "Обновить список"}
        </button>
      </div>
      {isPending && <p className="px-1.5 py-1 text-[12px] text-fg-subtle">Загрузка…</p>}
      {isError && <p className="px-1.5 py-1 text-[12px] text-warn">{String(error)}</p>}
      {data?.length === 0 && (
        <p className="px-1.5 py-1 text-[12px] leading-relaxed text-fg-subtle">
          Интеграции пока не открыта ни одна база. В Notion откройте базу → ··· → Connections → добавьте интеграцию,
          затем нажмите «Обновить список».
        </p>
      )}
      {data?.map((s) => {
        const active = notionSource?.id === s.id;
        return (
          <button
            key={s.id}
            onClick={() => setNotionSource(s)}
            className={clsx(
              "flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-left text-[13px]",
              active ? "bg-ink/10" : "hover:bg-ink/6",
            )}
          >
            <Database className="size-3.5 shrink-0 text-fg-subtle" />
            <span className="min-w-0 flex-1 truncate">{s.title}</span>
            {active && <Check className="size-4 text-accent" />}
          </button>
        );
      })}
    </div>
  );
}

/** The notes database: kept by the backend, which caches it for offline reading. */
function NotesSourcePicker() {
  const { data: notes } = useNotes();
  const { setSource } = useNotesActions();
  const { data } = useQuery({
    queryKey: ["notion-sources"],
    queryFn: () => invoke<Source[]>("notion_list_sources"),
    staleTime: 60_000,
  });
  const current = notes?.source?.id;

  return (
    <div className="flex flex-col gap-1 p-2">
      <div className="flex items-center justify-between px-1.5 pt-1 pb-1">
        <span className="text-[12px] text-fg-subtle">База заметок · хранится на компьютере для работы без интернета</span>
        {current && (
          <button onClick={() => setSource(null).catch(console.error)} className="text-[12px] text-fg-subtle hover:text-fg">
            Не выбирать
          </button>
        )}
      </div>
      {data?.map((s) => {
        const active = current === s.id;
        return (
          <button
            key={s.id}
            onClick={() => !active && setSource(s).catch(console.error)}
            className={clsx("flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-left text-[13px]", active ? "bg-ink/10" : "hover:bg-ink/6")}
          >
            <StickyNote className="size-3.5 shrink-0 text-fg-subtle" />
            <span className="min-w-0 flex-1 truncate">{s.title}</span>
            {active && <Check className="size-4 text-accent" />}
          </button>
        );
      })}
      {current && notes && (
        <p className="px-1.5 pt-1 text-[11.5px] leading-relaxed text-fg-subtle">
          {notes.syncing ? "Синхронизирую…" : `${notes.notes.length} заметок в кэше`}
          {notes.error && <span className="text-warn"> · {notes.error}</span>}
        </p>
      )}
    </div>
  );
}

export function NotionSettings({ tasksOnly = false }: { tasksOnly?: boolean }) {
  const queryClient = useQueryClient();
  const setNotionSource = usePrefs((s) => s.setNotionSource);
  const { data: status, isPending } = useNotionStatus();

  if (isPending) return <p className="p-3.5 text-[12px] text-fg-subtle">Проверяю подключение…</p>;
  if (!status?.connected) return <Connect />;

  const disconnect = async () => {
    await invoke("notion_disconnect");
    setNotionSource(null);
    queryClient.removeQueries({ queryKey: ["notion-tasks"] });
    queryClient.removeQueries({ queryKey: ["notion-sources"] });
    await queryClient.invalidateQueries({ queryKey: ["notion-status"] });
  };

  return (
    <div className="divide-y divide-stroke">
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div className="min-w-0">
          <div className="truncate text-[14px]">{status.workspace ?? "Notion"}</div>
          <div className={clsx("text-[12px]", status.error ? "text-warn" : "text-ok")}>
            {status.error ?? "Подключено"}
          </div>
        </div>
        <button className={button} onClick={disconnect}>
          Отключить
        </button>
      </div>
      <SourcePicker />
      {!tasksOnly && <NotesSourcePicker />}
    </div>
  );
}
