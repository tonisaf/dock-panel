import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Heart, ListMusic, Loader2, Play } from "lucide-react";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { playUri, useLibrary, useSpotifyStatus } from "./api";

const COLLAPSED = 7;

function Tile({
  name,
  subtitle,
  art,
  onPlay,
  busy,
}: {
  name: string;
  subtitle?: string;
  art: React.ReactNode;
  onPlay: () => void;
  busy: boolean;
}) {
  return (
    <button onClick={onPlay} title={name} className="group flex min-w-0 flex-col gap-1.5 text-left outline-none">
      <div className="relative aspect-square w-full overflow-hidden rounded-lg bg-ink/8 shadow-md shadow-black/30">
        {art}
        <div className="absolute inset-0 grid place-items-center bg-black/40 opacity-0 transition-opacity group-hover:opacity-100">
          <span className="grid size-8 place-items-center rounded-full bg-[#1ed760] text-black shadow-lg">
            {busy ? <Loader2 className="size-4 animate-spin" /> : <Play className="ml-0.5 size-4" fill="currentColor" />}
          </span>
        </div>
      </div>
      <div className="min-w-0 px-0.5">
        <div className="truncate text-[11.5px] leading-tight">{name}</div>
        {subtitle && <div className="truncate text-[10.5px] text-fg-subtle">{subtitle}</div>}
      </div>
    </button>
  );
}

export function PlaylistsWidget() {
  const queryClient = useQueryClient();
  const setTab = usePanelStore((s) => s.setTab);
  const status = useSpotifyStatus();
  const connected = !!status.data?.connected;
  const { data, isPending, isError, error } = useLibrary(connected);
  const [expanded, setExpanded] = useState(false);
  const [busyUri, setBusyUri] = useState<string | null>(null);

  if (!connected) {
    return (
      <Card title="Плейлисты" icon={ListMusic}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Войдите в Spotify в настройках →
        </button>
      </Card>
    );
  }

  const play = async (uri: string) => {
    setBusyUri(uri);
    try {
      const outcome = await playUri(uri);
      // Spotify came to the front either way; get out of its way.
      if (outcome !== "played") usePanelStore.getState().setOpen(false);
      setTimeout(() => queryClient.invalidateQueries({ queryKey: ["media"] }), 800);
    } catch (e) {
      console.error(e);
    } finally {
      setBusyUri(null);
    }
  };

  const playlists = data?.playlists ?? [];
  const shown = expanded ? playlists : playlists.slice(0, COLLAPSED);
  const tracks = (n: number | null) => (n == null ? undefined : `${n} треков`);

  return (
    <Card title="Плейлисты" icon={ListMusic}>
      {isError ? (
        <p className="text-[12px] leading-relaxed text-warn">{String(error)}</p>
      ) : isPending ? (
        <p className="text-[12px] text-fg-subtle">Загрузка…</p>
      ) : (
        <>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(80px,1fr))] gap-2.5">
            {data && (
              <Tile
                name="Любимые треки"
                subtitle={tracks(data.likedTotal)}
                busy={busyUri === data.likedUri}
                onPlay={() => play(data.likedUri)}
                art={
                  <div className="grid size-full place-items-center bg-gradient-to-br from-[#4b2fd8] to-[#8fc2b8]">
                    <Heart className="size-6 text-white" fill="currentColor" />
                  </div>
                }
              />
            )}
            {shown.map((p) => (
              <Tile
                key={p.id}
                name={p.name}
                subtitle={tracks(p.tracks)}
                busy={busyUri === p.uri}
                onPlay={() => play(p.uri)}
                art={
                  p.image ? (
                    <img src={p.image} className="size-full object-cover" draggable={false} loading="lazy" />
                  ) : (
                    <ListMusic className="m-auto mt-[35%] size-5 text-fg-subtle" />
                  )
                }
              />
            ))}
          </div>
          {playlists.length > COLLAPSED && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="mt-2.5 w-full rounded-lg py-1 text-[12px] text-fg-subtle hover:bg-ink/6 hover:text-fg"
            >
              {expanded ? "Свернуть" : `Все плейлисты · ${playlists.length}`}
            </button>
          )}
        </>
      )}
    </Card>
  );
}
