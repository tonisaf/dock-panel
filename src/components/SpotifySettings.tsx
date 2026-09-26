import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQueryClient } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, Loader2 } from "lucide-react";
import clsx from "clsx";
import { useSpotifyStatus } from "../widgets/spotify/api";

const REDIRECT_URI = "http://127.0.0.1:43821/callback";
const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

function CopyRedirect() {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        navigator.clipboard.writeText(REDIRECT_URI).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
      className="inline-flex items-center gap-1 rounded border border-stroke bg-field px-1.5 font-mono text-[11.5px] text-fg hover:bg-ink/8"
    >
      {REDIRECT_URI}
      {copied ? <Check className="size-3 text-ok" /> : <Copy className="size-3" />}
    </button>
  );
}

export function SpotifySettings() {
  const queryClient = useQueryClient();
  const { data: status, isPending } = useSpotifyStatus();
  const [clientId, setClientId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ["spotify-status"] }),
      queryClient.invalidateQueries({ queryKey: ["spotify-library"] }),
    ]);

  const login = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke<string>("spotify_login", { clientId });
      setClientId("");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const logout = async () => {
    await invoke("spotify_logout");
    queryClient.removeQueries({ queryKey: ["spotify-library"] });
    await refresh();
  };

  if (isPending) return <p className="p-3.5 text-[12px] text-fg-subtle">Проверяю подключение…</p>;

  if (status?.connected) {
    return (
      <div className="flex items-center justify-between gap-3 px-3.5 py-3">
        <div className="min-w-0">
          <div className="truncate text-[14px]">{status.user ?? "Spotify"}</div>
          <div className={clsx("text-[12px] leading-relaxed", status.error ? "text-warn" : "text-ok")}>
            {status.error ?? "Подключено"}
          </div>
          {!status.error && (!status.canLike || !status.canEditPlaylists) && (
            <div className="text-[12px] leading-relaxed text-warn">
              Чтобы ставить лайки и добавлять треки в плейлисты, выйдите и войдите снова
            </div>
          )}
        </div>
        <button className={button} onClick={logout}>
          Выйти
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2.5 p-3.5 text-[12.5px] leading-relaxed text-fg-muted">
      <ol className="list-decimal space-y-1.5 pl-4">
        <li>
          Откройте{" "}
          <button
            className="inline-flex items-center gap-0.5 text-accent hover:underline"
            onClick={() => openUrl("https://developer.spotify.com/dashboard").catch(console.error)}
          >
            Spotify Developer Dashboard <ExternalLink className="size-3" />
          </button>{" "}
          → Create app. В Redirect URIs добавьте <CopyRedirect />, в APIs отметьте Web API.
        </li>
        <li>Скопируйте Client ID приложения и вставьте ниже.</li>
        <li>Нажмите «Войти»: откроется браузер, разрешите доступ.</li>
      </ol>
      <p className="text-[11.5px] text-fg-subtle">
        Spotify пускает приложения в режиме разработки, только если у их владельца Premium. Панель читает плейлисты,
        запускает музыку, ставит лайки и переключает устройства; пароль и токены хранятся в Windows, не в панели.
      </p>
      <div className="flex gap-2">
        <input
          value={clientId}
          onChange={(e) => setClientId(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && clientId.trim() && login()}
          placeholder="Client ID"
          spellCheck={false}
          className="h-9 min-w-0 flex-1 rounded-lg border border-stroke bg-field px-2.5 font-mono text-[12.5px] text-fg outline-none placeholder:font-sans placeholder:text-fg-subtle focus:border-accent/50"
        />
        <button className={button} disabled={busy || !clientId.trim()} onClick={login}>
          {busy && <Loader2 className="size-3.5 animate-spin" />} {busy ? "Жду браузер…" : "Войти"}
        </button>
      </div>
      {error && <p className="text-warn">{error}</p>}
    </div>
  );
}
