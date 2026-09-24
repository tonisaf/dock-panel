import { forwardRef, type KeyboardEvent } from "react";
import { Search } from "lucide-react";
import { launchApp, useSearchResults } from "../lib/apps";
import { usePanelStore } from "../store";

export const SearchBar = forwardRef<HTMLInputElement>(function SearchBar(_, ref) {
  const { query, setQuery, tab, setTab, selected, setSelected } = usePanelStore();
  const results = useSearchResults();

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!results.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setSelected((selected + step + results.length) % results.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      launchApp(results[Math.min(selected, results.length - 1)].id);
    }
  };

  return (
    <label className="flex h-11 shrink-0 items-center gap-2.5 rounded-xl border border-stroke bg-surface px-3.5 transition-colors focus-within:border-accent/50 focus-within:bg-surface-hover">
      <Search className="size-4 text-fg-subtle" strokeWidth={2.2} />
      <input
        ref={ref}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          // Typing is almost always an app search.
          if (e.target.value && tab !== "apps") setTab("apps");
        }}
        onKeyDown={onKeyDown}
        placeholder="Поиск приложений"
        spellCheck={false}
        className="h-full flex-1 bg-transparent text-[15px] outline-none placeholder:text-fg-subtle"
      />
      <kbd className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Esc</kbd>
    </label>
  );
});
