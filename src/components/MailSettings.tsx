import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, Mail } from "lucide-react";
import { Toggle } from "./Toggle";
import { useMailActions, useMailSettings } from "../mail/api";

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";
const field =
  "h-9 min-w-0 rounded-lg border border-stroke bg-field px-2.5 text-[12.5px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50";

/** Domains the backend knows the IMAP server for. */
const KNOWN = /@(gmail\.com|googlemail\.com|yandex\.(ru|com|by|kz|ua)|ya\.ru|narod\.ru|mail\.ru|bk\.ru|inbox\.ru|list\.ru|internet\.ru|icloud\.com|me\.com|mac\.com)$/i;

function Link({ href, children }: { href: string; children: string }) {
  return (
    <button
      className="inline-flex items-center gap-0.5 text-accent hover:underline"
      onClick={() => openUrl(href).catch(console.error)}
    >
      {children} <ExternalLink className="size-3" />
    </button>
  );
}

function AddAccount({ onDone }: { onDone?: () => void }) {
  const { add } = useMailActions();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [host, setHost] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsHost = email.includes("@") && !KNOWN.test(email.trim());
  const isYandex = /@(yandex\.|ya\.ru|narod\.ru)/i.test(email);
  const isGmail = /@(gmail|googlemail)\.com$/i.test(email.trim());

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await add(email.trim(), password, needsHost ? host.trim() : null);
      setEmail("");
      setPassword("");
      setHost("");
      onDone?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <p>
        Нужен не обычный пароль, а <b className="font-medium text-fg">пароль приложения</b>:
      </p>
      <ul className="list-disc space-y-1 pl-4">
        <li className={isGmail ? "opacity-40" : undefined}>
          Яндекс: включите IMAP в <Link href="https://mail.yandex.ru/#setup/client">настройках почты</Link> (Почтовые
          программы), затем создайте пароль типа «Почта» в <Link href="https://id.yandex.ru/security/app-passwords">Яндекс ID</Link>.
        </li>
        <li className={isYandex ? "opacity-40" : undefined}>
          Gmail: нужна двухэтапная аутентификация, пароль создаётся на странице{" "}
          <Link href="https://myaccount.google.com/apppasswords">Пароли приложений</Link>.
        </li>
      </ul>
      <div className="flex flex-col gap-2">
        <input
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="Адрес, например me@yandex.ru"
          spellCheck={false}
          className={field}
        />
        <input
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && email && password && submit()}
          type="password"
          placeholder="Пароль приложения"
          className={field}
        />
        {needsHost && (
          <input
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="IMAP-сервер, например imap.example.com"
            spellCheck={false}
            className={field}
          />
        )}
        <button
          className={`${button} self-start`}
          disabled={busy || !email.includes("@") || !password || (needsHost && !host.trim())}
          onClick={submit}
        >
          {busy && <Loader2 className="size-3.5 animate-spin" />} {busy ? "Проверяю…" : "Добавить ящик"}
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
      <p className="text-[11.5px] text-fg-subtle">Пароль хранится в Windows (Диспетчер учётных данных), не в панели.</p>
    </div>
  );
}

export function MailSettings() {
  const { data } = useMailSettings();
  const { remove, setNotify } = useMailActions();
  const [adding, setAdding] = useState(false);
  const accounts = data?.accounts ?? [];

  if (!data) return <p className="p-3.5 text-[12px] text-fg-subtle">Загружаю…</p>;
  if (accounts.length === 0) return <AddAccount />;

  return (
    <div className="divide-y divide-stroke">
      {accounts.map((a) => (
        <div key={a.id} className="flex items-center justify-between gap-3 px-3.5 py-3">
          <div className="flex min-w-0 items-center gap-2.5">
            <Mail className="size-4 shrink-0 text-accent" />
            <div className="min-w-0">
              <div className="truncate text-[14px]">{a.email}</div>
              <div className="truncate text-[12px] text-fg-subtle">{a.host}</div>
            </div>
          </div>
          <button className={button} onClick={() => remove(a.id).catch(console.error)}>
            Удалить
          </button>
        </div>
      ))}
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div>
          <div className="text-[14px]">Уведомления о новых письмах</div>
          <div className="text-[12px] text-fg-subtle">Почта проверяется раз в минуту</div>
        </div>
        <Toggle on={data.notify} onChange={(on) => setNotify(on).catch(console.error)} />
      </div>
      {adding ? (
        <AddAccount onDone={() => setAdding(false)} />
      ) : (
        <div className="px-3.5 py-3">
          <button className={button} onClick={() => setAdding(true)}>
            Добавить ещё ящик
          </button>
        </div>
      )}
    </div>
  );
}
