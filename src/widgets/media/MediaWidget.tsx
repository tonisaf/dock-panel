import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Music2, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { AppIcon } from "../../components/AppIcon";
import { useAppsById } from "../../lib/apps";
import { SpotifyActions } from "./SpotifyActions";

interface NowPlaying {
  title: string;
  artist: string;
  album: string;
  source: string;
  playing: boolean;
  canPrev: boolean;
  canNext: boolean;
  positionMs: number | null;
  durationMs: number | null;
}

const POLL_MS = 1500;

/** Friendly player name from its AppUserModelID when it isn't in the app list. */
function playerName(source: string) {
  const s = source.toLowerCase();
  if (s.includes("spotify")) return "Spotify";
  if (s.includes("chrome")) return "Chrome";
  if (s.includes("msedge")) return "Edge";
  if (s.includes("firefox") || s.startsWith("308046b0af4a39cb")) return "Firefox";
  if (s.includes("yandex")) return "Яндекс";
  if (s.includes("vlc")) return "VLC";
  return source.replace(/\.exe$/i, "");
}

const fmt = (ms: number) => {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** Interpolates the playback position between polls. */
function useLivePosition(np: NowPlaying | null | undefined, fetchedAt: number) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!np?.playing) return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [np?.playing]);
  if (np?.positionMs == null || !np.durationMs) return null;
  const elapsed = np.playing ? Math.max(0, now - fetchedAt) : 0;
  return Math.min(np.positionMs + elapsed, np.durationMs);
}

export function MediaWidget() {
  const queryClient = useQueryClient();
  const apps = useAppsById();
  const { data: np, dataUpdatedAt } = useQuery({
    queryKey: ["media"],
    queryFn: () => invoke<NowPlaying | null>("media_now_playing"),
    refetchInterval: POLL_MS,
    staleTime: 0,
  });
  const trackKey = np ? `${np.source}|${np.title}|${np.artist}` : null;
  const { data: art } = useQuery({
    queryKey: ["media-art", trackKey],
    queryFn: () => invoke<string | null>("media_thumbnail"),
    enabled: !!trackKey,
    staleTime: Infinity,
  });
  const position = useLivePosition(np, dataUpdatedAt);

  const control = async (action: "toggle" | "next" | "prev") => {
    await invoke("media_control", { action }).catch(console.error);
    setTimeout(() => queryClient.invalidateQueries({ queryKey: ["media"] }), 250);
  };

  if (!np || !np.title) {
    return (
      <Card title="Сейчас играет" icon={Music2}>
        <p className="text-[12px] text-fg-subtle">Ничего не играет</p>
      </Card>
    );
  }

  const sourceApp = apps.get(np.source);
  const isSpotify = np.source.toLowerCase().includes("spotify");
  const btn = "grid place-items-center rounded-full transition-colors hover:bg-ink/10 disabled:opacity-30";

  return (
    <Card className="relative overflow-hidden">
      {art && (
        <img src={art} aria-hidden className="pointer-events-none absolute inset-0 size-full scale-150 object-cover opacity-25 blur-2xl" />
      )}
      <div className="relative flex items-center gap-3">
        <div className="size-16 shrink-0 overflow-hidden rounded-xl bg-ink/8 shadow-lg shadow-black/30">
          {art ? (
            <img src={art} className="size-full object-cover" draggable={false} />
          ) : (
            <Music2 className="m-auto mt-5 size-6 text-fg-subtle" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-[11px] text-fg-muted">
            {sourceApp ? <AppIcon id={sourceApp.id} size={14} /> : <Music2 className="size-3" />}
            {sourceApp?.name ?? playerName(np.source)}
          </div>
          <div className="mt-0.5 truncate text-[14px] font-semibold">{np.title}</div>
          <div className="truncate text-[12.5px] text-fg-muted">{np.artist || np.album}</div>
        </div>
        <div className="flex items-center gap-0.5">
          <button className={clsx(btn, "size-8")} disabled={!np.canPrev} onClick={() => control("prev")}>
            <SkipBack className="size-4" fill="currentColor" />
          </button>
          <button className={clsx(btn, "size-10 bg-ink/10")} onClick={() => control("toggle")}>
            {np.playing ? (
              <Pause className="size-5" fill="currentColor" />
            ) : (
              <Play className="ml-0.5 size-5" fill="currentColor" />
            )}
          </button>
          <button className={clsx(btn, "size-8")} disabled={!np.canNext} onClick={() => control("next")}>
            <SkipForward className="size-4" fill="currentColor" />
          </button>
        </div>
      </div>

      {(position != null && np.durationMs) || (isSpotify && trackKey) ? (
        <div className="relative mt-3 flex items-center gap-2 text-[11px] text-fg-subtle tabular-nums">
          {position != null && np.durationMs ? (
            <>
              <span>{fmt(position)}</span>
              <div className="h-1 flex-1 overflow-hidden rounded-full bg-ink/10">
                <div className="h-full rounded-full bg-fg/80" style={{ width: `${(position / np.durationMs) * 100}%` }} />
              </div>
              <span>{fmt(np.durationMs)}</span>
            </>
          ) : (
            <div className="flex-1" />
          )}
          {isSpotify && trackKey && <SpotifyActions trackKey={trackKey} />}
        </div>
      ) : null}
    </Card>
  );
}
