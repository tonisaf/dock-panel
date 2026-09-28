import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Music2, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { AppIcon } from "../../components/AppIcon";
import { useAppsById } from "../../lib/apps";
import { SpotifyActions } from "./SpotifyActions";
import { seekSpotify, useSpotifyStatus } from "../spotify/api";

interface NowPlaying {
  title: string;
  artist: string;
  album: string;
  source: string;
  playing: boolean;
  canPrev: boolean;
  canNext: boolean;
  canSeek: boolean;
  positionMs: number | null;
  durationMs: number | null;
}

const POLL_MS = 1500;
/** How long a seek's target is shown before trusting the player's position again. */
const SEEK_HOLD_MS = 2500;

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

/** How often the playback position moves on screen while playing. */
const TICK_MS = 1000;

/** Interpolates the playback position between polls. */
function useLivePosition(np: NowPlaying | null | undefined, fetchedAt: number) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!np?.playing) return;
    // A second is enough: the bar moves a pixel or two a tick, and a smooth
    // glide would redraw the whole acrylic panel every frame while playing.
    const id = setInterval(() => setNow(Date.now()), TICK_MS);
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
  const isSpotify = !!np?.source.toLowerCase().includes("spotify");
  const { data: art } = useQuery({
    queryKey: ["media-art", trackKey],
    queryFn: () => invoke<string | null>("media_thumbnail"),
    enabled: !!trackKey,
    staleTime: Infinity,
  });
  const livePosition = useLivePosition(np, dataUpdatedAt);
  const [seekedTo, setSeekedTo] = useState<{ ms: number; at: number } | null>(null);
  const { data: spotifyStatus } = useSpotifyStatus();
  const spotifyConnected = !!spotifyStatus?.connected && !spotifyStatus.error;
  // Players report the new position a little late; show the target meanwhile.
  const position =
    seekedTo && Date.now() - seekedTo.at < SEEK_HOLD_MS && np?.durationMs
      ? Math.min(seekedTo.ms + (np.playing ? Date.now() - seekedTo.at : 0), np.durationMs)
      : livePosition;

  const control = async (action: "toggle" | "next" | "prev") => {
    await invoke("media_control", { action }).catch(console.error);
    setTimeout(() => queryClient.invalidateQueries({ queryKey: ["media"] }), 250);
  };

  const seek = async (ms: number) => {
    setSeekedTo({ ms, at: Date.now() });
    try {
      // Spotify's own API seeks reliably; its Windows media session may not.
      if (isSpotify && spotifyConnected) await seekSpotify(ms);
      else await invoke("media_seek", { positionMs: Math.round(ms) });
    } catch (e) {
      console.error(e);
      setSeekedTo(null);
    }
    setTimeout(() => queryClient.invalidateQueries({ queryKey: ["media"] }), 400);
  };

  if (!np || !np.title) {
    return (
      <Card title="Сейчас играет" icon={Music2}>
        <p className="text-[12px] text-fg-subtle">Ничего не играет</p>
      </Card>
    );
  }

  const sourceApp = apps.get(np.source);
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

      {position != null && np.durationMs ? (
        <SeekBar
          position={position}
          duration={np.durationMs}
          canSeek={np.canSeek || (isSpotify && spotifyConnected)}
          onSeek={seek}
        />
      ) : null}
      {isSpotify && trackKey && <SpotifyActions trackKey={trackKey} />}
    </Card>
  );
}

/** Progress bar; click or drag to move through the track when the player allows it. */
function SeekBar({
  position,
  duration,
  canSeek,
  onSeek,
}: {
  position: number;
  duration: number;
  canSeek: boolean;
  onSeek: (ms: number) => void;
}) {
  const bar = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const at = (clientX: number) => {
    const r = bar.current!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  };
  const shown = drag ?? position;
  const pct = `${(shown / duration) * 100}%`;
  const active = drag != null;

  return (
    <div className="relative mt-3 flex items-center gap-2 text-[11px] text-fg-subtle tabular-nums">
      <span className="w-9">{fmt(shown)}</span>
      <div
        ref={bar}
        className={clsx("group flex h-3 flex-1 items-center", canSeek && "cursor-pointer")}
        onPointerDown={(e) => {
          if (!canSeek || e.button !== 0) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          setDrag(at(e.clientX));
        }}
        onPointerMove={(e) => active && setDrag(at(e.clientX))}
        onPointerUp={(e) => {
          if (!active) return;
          onSeek(at(e.clientX));
          setDrag(null);
        }}
        onPointerCancel={() => setDrag(null)}
      >
        <div
          className={clsx(
            "relative w-full rounded-full bg-ink/10 transition-[height]",
            active ? "h-1.5" : "h-1",
            canSeek && "group-hover:h-1.5",
          )}
        >
          <div
            className={clsx("h-full rounded-full", active ? "bg-accent" : "bg-fg/80", canSeek && "group-hover:bg-accent")}
            style={{ width: pct }}
          />
          {canSeek && (
            <div
              className={clsx(
                "absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg shadow transition-opacity",
                active ? "opacity-100" : "opacity-0 group-hover:opacity-100",
              )}
              style={{ left: pct }}
            />
          )}
        </div>
      </div>
      <span className="w-9 text-right">{fmt(duration)}</span>
    </div>
  );
}
