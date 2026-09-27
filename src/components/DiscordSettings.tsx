import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, ExternalLink, Loader2, Plus, Volume2, X } from "lucide-react";
import clsx from "clsx";
import { Toggle } from "./Toggle";
import { prettyAccelerator, toAccelerator } from "../lib/accelerator";
import { useDiscord, useDiscordActions, useGuilds, useVoiceChannels, type DiscordState, type WatchedChannel } from "../discord/api";

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
  const { login } = useDiscordActions();
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
        Кто сидит в выбранных голосовых каналах (с уведомлением, когда кто-то заходит), микрофон и звук — через запущенный на
        этом компьютере Discord. Сообщения панель не читает.
      </p>
      <ol className="list-decimal space-y-1.5 pl-4">
        <li>
          <Link href="https://discord.com/developers/applications">Создайте приложение Discord</Link> («New Application», имя
          любое). Управлять голосом Discord разрешает только приложению его владельца, поэтому нужно своё.
        </li>
        <li>
          В разделе OAuth2 добавьте Redirect <span className={code}>http://localhost</span>, нажмите «Reset Secret» и скопируйте
          Client ID и Client Secret сюда.
        </li>
        <li>Нажмите «Войти»: в самом Discord появится окно с запросом доступа — разрешите.</li>
      </ol>
      <p className="text-[11.5px] text-fg-subtle">
        Вход и обновление токена идут через discord.com — в России для этого нужен VPN, дальше всё работает локально. Токены
        хранятся в Windows.
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
          {busy && <Loader2 className="size-3.5 animate-spin" />} {busy ? "Подтвердите в Discord…" : "Войти"}
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}

/** Picks voice channels server by server. */
function ChannelPicker({ watched, onChange }: { watched: WatchedChannel[]; onChange: (w: WatchedChannel[]) => void }) {
  const guilds = useGuilds(true);
  const [guildId, setGuildId] = useState<string | null>(null);
  const channels = useVoiceChannels(guildId);
  const toggle = (c: WatchedChannel) =>
    onChange(watched.some((w) => w.id === c.id) ? watched.filter((w) => w.id !== c.id) : [...watched, c]);

  return (
    <div className="flex flex-col gap-2">
      <select
        value={guildId ?? ""}
        onChange={(e) => setGuildId(e.target.value || null)}
        className="w-full rounded-lg border border-stroke bg-field px-2.5 py-1.5 text-[13px] text-fg outline-none focus:border-accent/50"
      >
        <option value="">{guilds.isPending ? "Загружаю серверы…" : "Выберите сервер"}</option>
        {guilds.data?.map((g) => (
          <option key={g.id} value={g.id}>
            {g.name}
          </option>
        ))}
      </select>
      {guilds.error && <p className="text-[12px] text-warn">{String(guilds.error)}</p>}
      {channels.error && <p className="text-[12px] text-warn">{String(channels.error)}</p>}
      {channels.data && (
        <div className="flex flex-col">
          {channels.data.length === 0 && <p className="text-[12px] text-fg-subtle">На сервере нет голосовых каналов</p>}
          {channels.data.map((c) => {
            const on = watched.some((w) => w.id === c.id);
            return (
              <button key={c.id} onClick={() => toggle(c)} className="flex items-center gap-2 rounded-lg px-1.5 py-1 text-left text-[13px] hover:bg-ink/6">
                <span className={clsx("grid size-4 place-items-center rounded border", on ? "border-accent bg-accent text-on-accent" : "border-ink/25")}>
                  {on && <Check className="size-3" />}
                </span>
                <Volume2 className="size-3.5 text-fg-subtle" />
                <span className="truncate">{c.name}</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function MuteShortcut({ value }: { value: string | null }) {
  const { setMuteShortcut } = useDiscordActions();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setRecording(false);
      const accel = toAccelerator(e);
      if (!accel) return;
      setRecording(false);
      setMuteShortcut(accel).catch((err) => setError(String(err)));
    };
    // Capture phase: beat the panel's own Esc / Ctrl+1..5 handling.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="px-3.5 py-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="text-[14px]">Горячая клавиша микрофона</div>
          <div className="mt-0.5 text-[12px] text-fg-subtle">
            {recording ? "Нажмите сочетание, Esc — отмена" : "Включает и выключает микрофон из любого окна"}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={() => {
              setError(null);
              setRecording(!recording);
            }}
            className={clsx(
              "rounded-lg border px-2.5 py-1 font-mono text-[12px] transition-colors",
              recording ? "animate-pulse border-accent bg-accent/15 text-fg" : "border-stroke bg-surface text-fg-muted hover:text-fg",
            )}
          >
            {recording ? "…" : value ? prettyAccelerator(value) : "Задать"}
          </button>
          {value && !recording && (
            <button
              title="Убрать"
              onClick={() => setMuteShortcut(null).catch((err) => setError(String(err)))}
              className="grid size-7 place-items-center rounded-lg text-fg-subtle hover:bg-ink/8 hover:text-fg"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
      </div>
      {error && <p className="mt-1.5 text-[12px] text-warn">{error}</p>}
    </div>
  );
}

function Connected({ s }: { s: DiscordState }) {
  const { logout, setNotify, setWatched } = useDiscordActions();
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const watched: WatchedChannel[] = s.watched.map(({ members: _, ...c }) => c);
  const save = (w: WatchedChannel[]) => {
    setError(null);
    setWatched(w).catch((e) => setError(String(e)));
  };

  return (
    <div className="divide-y divide-stroke">
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div className="min-w-0">
          <div className="text-[14px]">{s.user?.name ?? "Discord"}</div>
          <div className={clsx("text-[12px] leading-relaxed", s.error ? "text-warn" : s.connected ? "text-ok" : "text-fg-subtle")}>
            {s.error ?? (s.connected ? "Подключено к Discord" : "Discord не запущен — подключусь, когда он откроется")}
          </div>
        </div>
        <button className={button} onClick={() => logout().catch(console.error)}>
          Выйти
        </button>
      </div>

      <div className="flex flex-col gap-2 px-3.5 py-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-[14px]">Каналы для слежения</div>
            <div className="mt-0.5 text-[12px] text-fg-subtle">Видно, кто в них сидит; клик в виджете — зайти</div>
          </div>
          {s.connected && (
            <button className={button} onClick={() => setAdding(!adding)}>
              {adding ? "Готово" : <><Plus className="size-3.5" /> Добавить</>}
            </button>
          )}
        </div>
        {watched.map((c) => (
          <div key={c.id} className="flex items-center gap-2 text-[13px]">
            <Volume2 className="size-3.5 shrink-0 text-fg-subtle" />
            <span className="truncate">{c.name}</span>
            <span className="truncate text-[12px] text-fg-subtle">{c.guildName}</span>
            <button
              title="Не следить"
              onClick={() => save(watched.filter((w) => w.id !== c.id))}
              className="ml-auto grid size-6 shrink-0 place-items-center rounded-md text-fg-subtle hover:bg-ink/8 hover:text-fg"
            >
              <X className="size-3.5" />
            </button>
          </div>
        ))}
        {adding && s.connected && <ChannelPicker watched={watched} onChange={save} />}
        {error && <p className="text-[12px] text-warn">{error}</p>}
      </div>

      <div className="flex items-center justify-between gap-4 px-3.5 py-3">
        <div>
          <div className="text-[14px]">Уведомлять о входе</div>
          <div className="mt-0.5 text-[12px] text-fg-subtle">Когда кто-то заходит в канал из списка; клик по уведомлению — зайти туда же</div>
        </div>
        <Toggle on={s.notify} onChange={(on) => setNotify(on).catch(console.error)} />
      </div>

      <MuteShortcut value={s.muteShortcut} />
    </div>
  );
}

export function DiscordSettings() {
  const { data: s, isPending } = useDiscord();
  if (isPending || !s) return <p className="p-3.5 text-[12px] text-fg-subtle">Проверяю подключение…</p>;
  return s.configured ? <Connected s={s} /> : <Connect />;
}
