import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2 } from "lucide-react";
import clsx from "clsx";
import { useYandexActions, useYandexStatus } from "../widgets/home/api";

const REDIRECT = "http://127.0.0.1:43822/callback";

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";
const field =
  "h-9 min-w-0 rounded-lg border border-stroke bg-field px-2.5 font-mono text-[12px] text-fg outline-none placeholder:font-sans placeholder:text-fg-subtle focus:border-accent/50";
const code = "rounded bg-ink/8 px-1 font-mono text-[11.5px] text-fg select-all";

function Link({ href, children }: { href: string; children: string }) {
  return (
    <button className="inline-flex items-center gap-0.5 text-accent hover:underline" onClick={() => openUrl(href).catch(console.error)}>
      {children} <ExternalLink className="size-3" />
    </button>
  );
}

function Connect() {
  const { login } = useYandexActions();
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
      <p>
        Лампы из «Дома с Алисой» — любые, что туда добавлены, в том числе через навыки производителей (Kojima и другие).
        Управление идёт через облако Яндекса.
      </p>
      <ol className="list-decimal space-y-1.5 pl-4">
        <li>
          Если лампы ещё нет в доме Алисы: приложение «Дом с Алисой» → «+» → «Устройство умного дома» → навык
          производителя → войдите его аккаунтом.
        </li>
        <li>
          <Link href="https://oauth.yandex.ru/client/new">Создайте приложение Яндекс OAuth</Link>: платформа «Веб-сервисы»,
          Redirect URI <span className={code}>{REDIRECT}</span>, доступы «Умный дом»: просмотр (
          <span className={code}>iot:view</span>) и управление (<span className={code}>iot:control</span>).
        </li>
        <li>Скопируйте ClientID и Client secret сюда, нажмите «Войти» и разрешите доступ.</li>
      </ol>
      <p className="text-[11.5px] text-fg-subtle">Токены хранятся в Windows, не в панели.</p>
      <div className="flex flex-col gap-2">
        <input value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="ClientID" spellCheck={false} className={field} />
        <input
          value={secret}
          onChange={(e) => setSecret(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && clientId && secret && submit()}
          placeholder="Client secret"
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

export function YandexSettings() {
  const { data: status, isPending } = useYandexStatus();
  const { logout } = useYandexActions();

  if (isPending) return <p className="p-3.5 text-[12px] text-fg-subtle">Проверяю подключение…</p>;
  if (!status?.connected) return <Connect />;

  return (
    <div className="flex items-center justify-between gap-3 px-3.5 py-3">
      <div className="min-w-0">
        <div className="text-[14px]">Дом с Алисой</div>
        <div className={clsx("text-[12px] leading-relaxed", status.error ? "text-warn" : "text-ok")}>
          {status.error ??
            (status.lamps
              ? "Лампы показаны в виджете «Дом»; лишние можно скрыть там же"
              : "Подключено, но ламп в доме Яндекса нет")}
        </div>
      </div>
      <button className={button} onClick={() => logout().catch(console.error)}>
        Выйти
      </button>
    </div>
  );
}
