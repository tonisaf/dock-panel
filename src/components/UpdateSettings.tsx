import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Loader2, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { useUpdateActions, useUpdateStatus } from "../lib/updates";

const TOKEN_URL = "https://github.com/settings/personal-access-tokens/new";
const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

function TokenForm() {
  const queryClient = useQueryClient();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("update_set_token", { token });
      setToken("");
      queryClient.setQueryData(["update-status"], await invoke("update_check"));
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
          Создайте токен на{" "}
          <button
            className="inline-flex items-center gap-0.5 text-accent hover:underline"
            onClick={() => openUrl(TOKEN_URL).catch(console.error)}
          >
            GitHub → Fine-grained token <ExternalLink className="size-3" />
          </button>
          : Repository access → Only select repositories → <b className="font-medium text-fg">dock-panel</b>; Permissions →
          Contents: <b className="font-medium text-fg">Read-only</b>. Срок действия — на ваш выбор.
        </li>
        <li>Вставьте токен сюда. Он хранится в диспетчере учётных данных Windows.</li>
      </ol>
      <div className="flex gap-2">
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && token.trim() && save()}
          placeholder="github_pat_…"
          spellCheck={false}
          className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 text-[13px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50"
        />
        <button className={button} disabled={busy || !token.trim()} onClick={save}>
          {busy && <Loader2 className="size-3.5 animate-spin" />} Сохранить
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}

export function UpdateSettings() {
  const queryClient = useQueryClient();
  const { data: status } = useUpdateStatus();
  const { check, install, installing, progress, error } = useUpdateActions();
  const [checking, setChecking] = useState(false);

  if (!status) return null;
  if (!status.hasToken) return <TokenForm />;

  const runCheck = async () => {
    setChecking(true);
    await check().finally(() => setChecking(false));
  };

  const message = error ?? status.error;

  return (
    <div className="flex flex-col gap-2 px-3.5 py-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[14px]">Версия {status.current}</div>
          <div className={clsx("text-[12px]", status.available ? "text-accent" : "text-fg-subtle")}>
            {status.available ? `Доступна ${status.available.version}` : "Проверка раз в 6 часов"}
          </div>
        </div>
        {status.available ? (
          <button className={button} disabled={installing} onClick={install}>
            {installing && <Loader2 className="size-3.5 animate-spin" />}
            {installing ? (progress != null ? `${progress}%` : "Скачиваю…") : "Обновить"}
          </button>
        ) : (
          <button className={button} disabled={checking} onClick={runCheck}>
            <RefreshCw className={clsx("size-3.5", checking && "animate-spin")} /> Проверить
          </button>
        )}
      </div>
      {status.available?.notes && (
        <p className="text-[12px] leading-relaxed whitespace-pre-line text-fg-muted">{status.available.notes}</p>
      )}
      {message && <p className="text-[12px] leading-relaxed text-warn">{message}</p>}
      <button
        onClick={async () => {
          await invoke("update_clear_token");
          queryClient.invalidateQueries({ queryKey: ["update-status"] });
        }}
        className="self-start text-[12px] text-fg-subtle hover:text-fg"
      >
        Удалить токен GitHub
      </button>
    </div>
  );
}
