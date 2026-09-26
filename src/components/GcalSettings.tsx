import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2 } from "lucide-react";
import clsx from "clsx";
import { useCalendars, useGcalActions, useGcalStatus } from "../gcal/api";

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";
const field =
  "h-9 min-w-0 rounded-lg border border-stroke bg-field px-2.5 font-mono text-[12px] text-fg outline-none placeholder:font-sans placeholder:text-fg-subtle focus:border-accent/50";

function Link({ href, children }: { href: string; children: string }) {
  return (
    <button className="inline-flex items-center gap-0.5 text-accent hover:underline" onClick={() => openUrl(href).catch(console.error)}>
      {children} <ExternalLink className="size-3" />
    </button>
  );
}

function Connect() {
  const { login } = useGcalActions();
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await login(clientId.trim(), secret.trim());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <ol className="list-decimal space-y-1.5 pl-4">
        <li>
          В <Link href="https://console.cloud.google.com/projectcreate">Google Cloud Console</Link> создайте проект и
          включите в нём <Link href="https://console.cloud.google.com/apis/library/calendar-json.googleapis.com">Calendar API</Link>{" "}
          и <Link href="https://console.cloud.google.com/apis/library/tasks.googleapis.com">Tasks API</Link>, а для
          подписок YouTube — <Link href="https://console.cloud.google.com/apis/library/youtube.googleapis.com">YouTube Data API v3</Link>.
        </li>
        <li>
          <Link href="https://console.cloud.google.com/auth/overview">Google Auth Platform</Link>: тип «External», добавьте себя
          в тестовые пользователи. Затем в «Audience» нажмите <b className="font-medium text-fg">Publish app</b> — иначе Google
          сбрасывает вход каждые 7 дней.
        </li>
        <li>
          <Link href="https://console.cloud.google.com/auth/clients/create">Clients → Create client</Link>, тип{" "}
          <b className="font-medium text-fg">Desktop app</b>. Скопируйте Client ID и Client Secret сюда.
        </li>
        <li>
          Нажмите «Войти» и разрешите доступ. Google предупредит, что приложение не проверено — это ваше же приложение:
          «Дополнительно» → «Перейти».
        </li>
      </ol>
      <p className="text-[11.5px] text-fg-subtle">
        Панель просит доступ к календарям (чтение и правка событий), к Google Tasks и чтение YouTube (подписки,
        длительность видео). Токены хранятся в Windows, не в панели.
      </p>
      <div className="flex flex-col gap-2">
        <input value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="Client ID" spellCheck={false} className={field} />
        <input
          value={secret}
          onChange={(e) => setSecret(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && clientId && secret && submit()}
          placeholder="Client Secret"
          type="password"
          spellCheck={false}
          className={field}
        />
        <button className={clsx(button, "self-start")} disabled={busy || !clientId.trim() || !secret.trim()} onClick={submit}>
          {busy && <Loader2 className="size-3.5 animate-spin" />} {busy ? "Жду браузер…" : "Войти"}
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}

export function GcalSettings() {
  const { data: status, isPending } = useGcalStatus();
  const connected = !!status?.connected;
  const { data: calendars = [] } = useCalendars(connected && !status?.error);
  const { logout, setVisible } = useGcalActions();

  if (isPending) return <p className="p-3.5 text-[12px] text-fg-subtle">Проверяю подключение…</p>;
  if (!connected) return <Connect />;

  return (
    <div className="divide-y divide-stroke">
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div className="min-w-0">
          <div className="truncate text-[14px]">{status?.email ?? "Google"}</div>
          <div className={clsx("text-[12px] leading-relaxed", status?.error ? "text-warn" : "text-ok")}>
            {status?.error ?? "Подключено"}
          </div>
        </div>
        <button className={button} onClick={() => logout().catch(console.error)}>
          Выйти
        </button>
      </div>
      {calendars.length > 0 && (
        <div className="flex flex-col px-2 py-1.5">
          <div className="px-1.5 pt-1 pb-0.5 text-[11.5px] text-fg-subtle">Показывать календари</div>
          {calendars.map((c) => (
            <label key={c.id} className="flex cursor-pointer items-center gap-2.5 rounded-lg px-1.5 py-1 hover:bg-ink/5">
              <input
                type="checkbox"
                checked={c.visible}
                onChange={(e) => setVisible(c.id, e.target.checked).catch(console.error)}
                className="size-4"
                style={{ accentColor: c.color }}
              />
              <span className="min-w-0 flex-1 truncate text-[13px]">{c.name}</span>
              {!c.writable && <span className="text-[11px] text-fg-subtle">только чтение</span>}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
