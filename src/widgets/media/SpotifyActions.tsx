import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Heart,
  ListMusic,
  ListPlus,
  Loader2,
  Music2,
  Repeat,
  Repeat1,
  Shuffle,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../../store";
import {
  addToPlaylist,
  setLiked,
  skipAhead,
  useLibrary,
  usePlayerActions,
  usePlayerState,
  useSpotifyCurrent,
  useSpotifyStatus,
  useUpNext,
  type CurrentTrack,
  type Repeat as RepeatMode,
} from "../spotify/api";
import { DevicePicker } from "../spotify/DevicePicker";
import { AnchoredMenu, menuItem } from "../spotify/Menu";

const GREEN = "text-[#1ed760]";
const icon = "grid size-7 place-items-center rounded-full transition-colors hover:bg-ink/10";
const idle = "text-fg-muted hover:text-fg";
const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "context", context: "track", track: "off" };
const REPEAT_TITLE: Record<RepeatMode, string> = {
  off: "Повтор выключен",
  context: "Повторять плейлист",
  track: "Повторять трек",
};

/** A short-lived note under the controls (errors, "added"). */
function useNote() {
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  useEffect(() => {
    if (!note) return;
    const id = setTimeout(() => setNote(null), note.error ? 5000 : 2000);
    return () => clearTimeout(id);
  }, [note]);
  return {
    note,
    ok: (text: string) => setNote({ text, error: false }),
    fail: (e: unknown) => setNote({ text: String(e), error: true }),
  };
}

/**
 * Spotify's own controls under the "now playing" card: shuffle, repeat,
 * volume, up next, add to playlist, like and device.
 */
export function SpotifyActions({ trackKey }: { trackKey: string }) {
  const { data: status } = useSpotifyStatus();
  const connected = !!status?.connected && !status.error;
  const { data: track } = useSpotifyCurrent(trackKey, connected);
  const { data: player } = usePlayerState(trackKey, connected);
  const actions = usePlayerActions(trackKey);
  const [queueOpen, setQueueOpen] = useState(false);
  const { note, ok, fail } = useNote();
  if (!connected) return null;

  const run = (p: Promise<unknown>) => p.catch(fail);

  return (
    <div className="relative mt-2">
      <div className="flex items-center gap-0.5">
        {player && (
          <>
            <button
              className={clsx(icon, player.shuffle ? GREEN : idle)}
              title={player.shuffle ? "Перемешивание включено" : "Перемешать"}
              onClick={() => run(actions.setShuffle(!player.shuffle))}
            >
              <Shuffle className="size-4" />
            </button>
            <button
              className={clsx(icon, player.repeat !== "off" ? GREEN : idle)}
              title={REPEAT_TITLE[player.repeat]}
              onClick={() => run(actions.setRepeat(NEXT_REPEAT[player.repeat]))}
            >
              {player.repeat === "track" ? <Repeat1 className="size-4" /> : <Repeat className="size-4" />}
            </button>
            {player.volume != null && (
              <VolumeControl volume={player.volume} onChange={(v) => run(actions.setVolume(v))} />
            )}
          </>
        )}
        <div className="flex-1" />
        <button
          className={clsx(icon, queueOpen ? "bg-ink/10 text-fg" : idle)}
          title="Далее в очереди"
          onClick={() => setQueueOpen(!queueOpen)}
        >
          <ListMusic className="size-4" />
        </button>
        {track && (
          <PlaylistPicker track={track} canEdit={status.canEditPlaylists} onDone={ok} onError={fail} />
        )}
        <LikeButton trackKey={trackKey} track={track} canLike={status.canLike} onError={fail} />
        <DevicePicker />
      </div>

      {note && (
        <p className={clsx("mt-1 truncate text-[11.5px]", note.error ? "text-warn" : "text-fg-muted")}>{note.text}</p>
      )}

      <AnimatePresence initial={false}>
        {queueOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18 }}
            className="overflow-hidden"
          >
            <UpNext trackKey={trackKey} onError={fail} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function LikeButton({
  trackKey,
  track,
  canLike,
  onError,
}: {
  trackKey: string;
  track: CurrentTrack | null | undefined;
  canLike: boolean;
  onError: (e: unknown) => void;
}) {
  const queryClient = useQueryClient();
  if (!track && canLike) return null;

  const toggle = async () => {
    if (!canLike) {
      usePanelStore.getState().setTab("settings");
      return;
    }
    if (!track) return;
    const liked = !track.liked;
    // Optimistic: the heart fills at once, and rolls back if Spotify refuses.
    queryClient.setQueryData(["spotify-current", trackKey], { ...track, liked });
    try {
      await setLiked(track, liked);
      queryClient.invalidateQueries({ queryKey: ["spotify-library"] });
    } catch (e) {
      queryClient.setQueryData(["spotify-current", trackKey], track);
      onError(e);
    }
  };

  return (
    <button
      onClick={toggle}
      title={
        !canLike
          ? "Чтобы ставить лайки, выйдите из Spotify в настройках и войдите снова"
          : track?.liked
            ? "Убрать из «Любимых треков»"
            : "Добавить в «Любимые треки»"
      }
      className={clsx(icon, track?.liked ? GREEN : idle, !canLike && "text-warn")}
    >
      <Heart className="size-4" fill={track?.liked ? "currentColor" : "none"} />
    </button>
  );
}

/** Mute button and a slider; the slider sends its value a moment after it settles. */
function VolumeControl({ volume, onChange }: { volume: number; onChange: (v: number) => void }) {
  const [draft, setDraft] = useState<number | null>(null);
  const beforeMute = useRef(50);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const shown = draft ?? volume;

  const send = (v: number) => {
    clearTimeout(timer.current);
    onChange(v);
    setDraft(null);
  };
  const change = (v: number) => {
    setDraft(v);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => send(v), 250);
  };
  useEffect(() => () => clearTimeout(timer.current), []);

  const Icon = shown === 0 ? VolumeX : shown < 50 ? Volume1 : Volume2;
  return (
    <div className="ml-1 flex items-center gap-1">
      <button
        className={clsx(icon, idle)}
        title={shown === 0 ? "Включить звук" : "Выключить звук"}
        onClick={() => {
          if (shown > 0) {
            beforeMute.current = shown;
            send(0);
          } else {
            send(beforeMute.current || 50);
          }
        }}
      >
        <Icon className="size-4" />
      </button>
      <input
        type="range"
        min={0}
        max={100}
        value={shown}
        title={`Громкость ${shown}%`}
        onChange={(e) => change(Number(e.target.value))}
        onPointerUp={(e) => send(Number((e.target as HTMLInputElement).value))}
        onKeyUp={(e) => send(Number((e.target as HTMLInputElement).value))}
        className="h-1 w-20 cursor-pointer accent-[#1ed760]"
      />
    </div>
  );
}

function PlaylistPicker({
  track,
  canEdit,
  onDone,
  onError,
}: {
  track: CurrentTrack;
  canEdit: boolean;
  onDone: (text: string) => void;
  onError: (e: unknown) => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const { data: library, isPending } = useLibrary(open);
  const playlists = library?.playlists.filter((p) => p.editable) ?? [];

  const add = async (id: string, name: string) => {
    setBusy(id);
    try {
      await addToPlaylist(id, track.uri);
      setOpen(false);
      onDone(`Добавлено в «${name}»`);
    } catch (e) {
      onError(e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <button
        ref={button}
        className={clsx(icon, open ? "bg-ink/10 text-fg" : idle, !canEdit && "text-warn")}
        title={
          canEdit
            ? "Добавить в плейлист"
            : "Чтобы добавлять в плейлисты, выйдите из Spotify в настройках и войдите снова"
        }
        onClick={() => (canEdit ? setOpen(!open) : usePanelStore.getState().setTab("settings"))}
      >
        <ListPlus className="size-4" />
      </button>
      {open && (
        <AnchoredMenu anchor={button} onClose={close} className="w-64">
          <div className="truncate px-2.5 pt-1 pb-1.5 text-[11px] text-fg-subtle">Добавить «{track.name}» в плейлист</div>
          <div className="max-h-72 overflow-y-auto">
            {isPending && <div className="px-2.5 py-1.5 text-[12px] text-fg-subtle">Загружаю плейлисты…</div>}
            {!isPending && playlists.length === 0 && (
              <div className="px-2.5 py-1.5 text-[12px] text-fg-subtle">Нет ваших плейлистов</div>
            )}
            {playlists.map((p) => (
              <button key={p.id} className={menuItem} disabled={!!busy} onClick={() => add(p.id, p.name)}>
                <span className="grid size-7 shrink-0 place-items-center overflow-hidden rounded bg-ink/8">
                  {p.image ? <img src={p.image} className="size-full object-cover" /> : <Music2 className="size-3.5 text-fg-subtle" />}
                </span>
                <span className="min-w-0 flex-1 truncate">{p.name}</span>
                {busy === p.id && <Loader2 className="size-3.5 animate-spin text-fg-subtle" />}
              </button>
            ))}
          </div>
        </AnchoredMenu>
      )}
    </>
  );
}

function UpNext({ trackKey, onError }: { trackKey: string; onError: (e: unknown) => void }) {
  const queryClient = useQueryClient();
  const { data: items, isPending } = useUpNext(trackKey, true);
  const [jumping, setJumping] = useState<number | null>(null);

  const jump = async (i: number) => {
    setJumping(i);
    try {
      await skipAhead(i + 1);
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ["media"] });
        queryClient.invalidateQueries({ queryKey: ["spotify-up-next"] });
      }, 400);
    } catch (e) {
      onError(e);
    } finally {
      setJumping(null);
    }
  };

  return (
    <div className="mt-2 border-t border-ink/10 pt-2">
      <div className="px-1 pb-1 text-[11px] text-fg-subtle">Далее</div>
      {isPending && <div className="px-1 py-1 text-[12px] text-fg-subtle">Загружаю очередь…</div>}
      {items?.length === 0 && <div className="px-1 py-1 text-[12px] text-fg-subtle">Очередь пуста</div>}
      <div className="max-h-56 overflow-y-auto">
        {items?.map((t, i) => (
          <button
            key={`${i}/${t.uri}`}
            disabled={jumping != null}
            onClick={() => jump(i)}
            title="Включить"
            className="flex w-full items-center gap-2.5 rounded-lg px-1 py-1 text-left hover:bg-ink/10"
          >
            <span className="grid size-8 shrink-0 place-items-center overflow-hidden rounded bg-ink/8">
              {jumping === i ? (
                <Loader2 className="size-3.5 animate-spin text-fg-subtle" />
              ) : t.image ? (
                <img src={t.image} className="size-full object-cover" />
              ) : (
                <Music2 className="size-3.5 text-fg-subtle" />
              )}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[12.5px]">{t.name}</span>
              <span className="block truncate text-[11.5px] text-fg-muted">{t.subtitle}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

