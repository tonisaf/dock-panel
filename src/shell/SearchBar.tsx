import { forwardRef, type KeyboardEvent } from "react";
import { Search } from "lucide-react";
import { launchApp, useSearchResults } from "../lib/apps";
import { usePanelStore } from "../store";
import { activateSpotifyItem, useSpotifySearch } from "../widgets/spotify/SpotifySearch";
import { useNoteSearch } from "../notes/api";

export const SearchBar = forwardRef<HTMLInputElement>(function SearchBar(_, ref) {
  const { query, setQuery, tab, setTab, selected, setSelected, openNote } = usePanelStore();
  const apps = useSearchResults();
  const spotify = useSpotifySearch();
  // Notes come after the apps in the results.
  const notes = useNoteSearch(spotify.term ? "" : query).data ?? [];
  // "sp <query>" searches Spotify instead of apps.
  const count = spotify.term ? spotify.results.length : apps.length + notes.length;

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!count) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setSelected((selected + step + count) % count);
    } else if (e.key === "Enter") {
      e.preventDefault();
      // The list still shows the previous term's results until the debounce settles.
      if (spotify.term && (spotify.typing || spotify.isFetching)) return;
      const i = Math.min(selected, count - 1);
      if (spotify.term) activateSpotifyItem(spotify.results[i], e.shiftKey);
      else if (i < apps.length) launchApp(apps[i].id);
      else openNote(notes[i - apps.length].id);
    }
  };

  return (
    <label className="flex h-11 min-w-0 flex-1 items-center gap-2.5 rounded-xl border border-stroke bg-surface px-3.5 transition-colors focus-within:border-accent/50 focus-within:bg-surface-hover">
      <Search className="size-4 text-fg-subtle" strokeWidth={2.2} />
      <input
        ref={ref}
        data-panel-search
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          // Typing is almost always an app search.
          if (e.target.value && tab !== "apps") setTab("apps");
        }}
        onKeyDown={onKeyDown}
        placeholder="Приложения, файлы, заметки · sp … — Spotify"
        spellCheck={false}
        className="h-full flex-1 bg-transparent text-[15px] outline-none placeholder:text-fg-subtle"
      />
      <kbd className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Esc</kbd>
    </label>
  );
});
