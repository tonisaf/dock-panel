import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { create } from "zustand";
import { Disc3, ListMusic, ListPlus, Loader2, Music2, SearchX, User } from "lucide-react";
import clsx from "clsx";
import { EmptyState } from "../../components/Card";
import { usePanelStore } from "../../store";
import { playItem, queueTrack, searchSpotify, spotifyTerm, useSpotifyStatus, type SearchItem } from "./api";
import { Cover } from "./Cover";

/** Results for "sp <query>" typed into the panel's search bar. */
export function useSpotifySearch() {
  const term = spotifyTerm(usePanelStore((s) => s.query));
  const [debounced, setDebounced] = useState(term);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(term), 300);
    return () => clearTimeout(id);
  }, [term]);
  const { data: status } = useSpotifyStatus();
  const connected = !!status?.connected;
  const query = useQuery({
    queryKey: ["spotify-search", debounced],
    queryFn: () => searchSpotify(debounced!),
    enabled: connected && !!debounced,
    staleTime: 5 * 60_000,
  });
  return { term, connected, results: query.data ?? [], ...query, typing: term !== debounced };
}

/** Which row is busy, and a one-line note under the results ("Добавлено в очередь"). */
const useFeedback = create<{ busy: string | null; note: { text: string; error: boolean } | null }>(() => ({
  busy: null,
  note: null,
}));

/** Enter plays (and closes the panel); Shift+Enter or the ＋ button queues a track. */
export async function activateSpotifyItem(item: SearchItem, queue: boolean) {
  if (queue && item.kind !== "track") return;
  useFeedback.setState({ busy: item.uri, note: null });
  try {
    if (queue) {
      await queueTrack(item.uri);
      useFeedback.setState({ note: { text: `В очереди: ${item.name}`, error: false } });
    } else {
      await playItem(item);
      usePanelStore.getState().setOpen(false);
    }
  } catch (e) {
    useFeedback.setState({ note: { text: String(e), error: true } });
  } finally {
    useFeedback.setState({ busy: null });
  }
}

const KIND_ICON = { track: Music2, artist: User, album: Disc3, playlist: ListMusic };

function Row({ item, active, onHover }: { item: SearchItem; active: boolean; onHover: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const busy = useFeedback((s) => s.busy === item.uri);
  const Icon = KIND_ICON[item.kind];
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div
      ref={ref}
      role="button"
      onClick={() => activateSpotifyItem(item, false)}
      onMouseMove={onHover}
      className={clsx(
        "flex w-full cursor-default items-center gap-3 rounded-xl px-2.5 py-1.5 text-left transition-colors",
        active ? "bg-ink/10" : "hover:bg-surface",
      )}
    >
      <div
        className={clsx(
          "grid size-9 shrink-0 place-items-center overflow-hidden bg-ink/8",
          item.kind === "artist" ? "rounded-full" : "rounded-md",
        )}
      >
        <Cover src={item.image} fallback={<Icon className="size-4 text-fg-subtle" />} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13.5px]">{item.name}</div>
        <div className="truncate text-[11.5px] text-fg-subtle">{item.subtitle}</div>
      </div>
      {busy && <Loader2 className="size-4 animate-spin text-fg-subtle" />}
      {item.kind === "track" && !busy && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            activateSpotifyItem(item, true);
          }}
          title="В очередь (Shift+Enter)"
          className="grid size-7 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg"
        >
          <ListPlus className="size-4" />
        </button>
      )}
      {active && !busy && (
        <kbd className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Enter</kbd>
      )}
    </div>
  );
}

export function SpotifyResults() {
  const { connected, results, isFetching, isError, error, typing, term } = useSpotifySearch();
  const { selected, setSelected, setTab } = usePanelStore();
  const note = useFeedback((s) => s.note);

  // A new query clears the previous note.
  useEffect(() => useFeedback.setState({ note: null }), [term]);

  if (!connected) {
    return (
      <div className="flex h-full flex-col">
        <EmptyState icon={Music2} title="Spotify не подключён" text="Войдите в Spotify в настройках, и здесь появится поиск." />
        <button onClick={() => setTab("settings")} className="mx-auto -mt-12 text-[12.5px] text-accent hover:underline">
          Открыть настройки
        </button>
      </div>
    );
  }
  if (isError) return <EmptyState icon={SearchX} title="Поиск не удался" text={String(error)} />;
  if (results.length === 0) {
    return (typing || isFetching) ? (
      <div className="flex items-center gap-2 px-2.5 py-3 text-[12.5px] text-fg-subtle">
        <Loader2 className="size-4 animate-spin" /> Ищу в Spotify…
      </div>
    ) : (
      <EmptyState icon={SearchX} title="Ничего не найдено" text="Попробуйте другое название." />
    );
  }

  return (
    <div className="flex flex-col gap-0.5 pb-2">
      <div className="px-2.5 pb-1 text-[11.5px] text-fg-subtle">
        Spotify · Enter — играть, Shift+Enter — в очередь
      </div>
      {results.map((item, i) => (
        <Row key={item.uri} item={item} active={i === selected} onHover={() => i !== selected && setSelected(i)} />
      ))}
      {note && (
        <div className={clsx("px-2.5 pt-2 text-[12px]", note.error ? "text-warn" : "text-ok")}>{note.text}</div>
      )}
    </div>
  );
}
