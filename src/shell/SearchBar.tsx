import { forwardRef, type KeyboardEvent } from "react";
import { CircleHelp, Search } from "lucide-react";
import { activateItem, useSearchItems } from "../lib/search";
import { usePanelStore } from "../store";
import { activateSpotifyItem, useSpotifySearch } from "../widgets/spotify/SpotifySearch";

export const SearchBar = forwardRef<HTMLInputElement>(function SearchBar(_, ref) {
  const { query, setQuery, tab, setTab, selected, setSelected } = usePanelStore();
  const items = useSearchItems();
  const spotify = useSpotifySearch();
  // "sp <query>" searches Spotify instead.
  const count = spotify.term ? spotify.results.length : items.length;

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
      else activateItem(items[i], { admin: e.ctrlKey && e.shiftKey });
    }
  };

  return (
    <div className="relative min-w-0 flex-1">
    <div className="flex h-11 min-w-0 items-center gap-2.5 rounded-xl border border-stroke bg-surface px-3.5 transition-colors focus-within:border-accent/50 focus-within:bg-surface-hover">
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
        placeholder="Поиск приложений, файлов и заметок"
        aria-label="Поиск приложений, файлов и заметок"
        spellCheck={false}
        className="h-full min-w-0 flex-1 bg-transparent text-[15px] outline-none placeholder:text-fg-subtle"
      />
      <kbd className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Esc</kbd>
      <div className="group/help relative shrink-0">
        <button
          type="button"
          aria-label="Подсказка по командам поиска"
          aria-describedby="search-command-hints"
          className="grid size-7 place-items-center rounded-md text-fg-subtle transition-colors hover:bg-ink/8 hover:text-fg-muted focus-visible:outline-2 focus-visible:outline-accent"
        >
          <CircleHelp className="size-4" strokeWidth={1.8} />
        </button>
        <div
          id="search-command-hints"
          role="tooltip"
          className="absolute top-full right-0 z-30 hidden w-64 max-w-[calc(100vw-3rem)] pt-2 group-hover/help:block group-focus-within/help:block"
        >
          <div className="rounded-xl border border-stroke bg-popover px-3 py-2.5 text-[12px] leading-relaxed text-fg-muted shadow-md">
            <div className="mb-1.5 font-medium text-fg">Команды поиска</div>
            <div className="space-y-1">
              <div><code className="text-fg">2+2</code> — калькулятор</div>
              <div><code className="text-fg">100 usd</code> — курс валют</div>
              <div><code className="text-fg">sp название</code> — поиск в Spotify</div>
            </div>
          </div>
        </div>
      </div>
    </div>
    </div>
  );
});
