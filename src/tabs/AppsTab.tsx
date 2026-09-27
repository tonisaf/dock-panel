import { useEffect, useMemo, useRef, type ReactNode } from "react";
import { SearchX, StickyNote } from "lucide-react";
import clsx from "clsx";
import { useNoteSearch } from "../notes/api";
import { useApps, useAppsById, usePinnedEntries, useSearchResults, type AppEntry } from "../lib/apps";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { AppGrid, AppRow } from "../components/AppTile";
import { EmptyState } from "../components/Card";
import { PinFromDisk } from "../components/PinFromDisk";
import { spotifyTerm } from "../widgets/spotify/api";
import { SpotifyResults } from "../widgets/spotify/SpotifySearch";

const RECENT_COUNT = 8;

function Section({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="flex items-center px-1 text-[12px] font-medium text-fg-subtle">
        {title}
        {action && <span className="ml-auto flex items-center gap-0.5">{action}</span>}
      </h3>
      {children}
    </section>
  );
}

function SearchResults() {
  const results = useSearchResults();
  const { selected, setSelected, query, openNote } = usePanelStore();
  const notes = useNoteSearch(query).data ?? [];
  const rows = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    rows.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (results.length === 0 && notes.length === 0) {
    return <EmptyState icon={SearchX} title="Ничего не найдено" text="Попробуйте другое название или первые буквы слов." />;
  }
  return (
    <div className="flex flex-col gap-0.5 pb-2">
      {results.map((app, i) => (
        <AppRow
          key={app.id}
          ref={(el) => {
            rows.current[i] = el;
          }}
          app={app}
          active={i === selected}
          onHover={() => i !== selected && setSelected(i)}
        />
      ))}
      {notes.length > 0 && <h3 className="px-1 pt-2 pb-1 text-[12px] font-medium text-fg-subtle">Заметки</h3>}
      {notes.map((n, j) => {
        const i = results.length + j;
        return (
          <button
            key={n.id}
            ref={(el) => {
              rows.current[i] = el;
            }}
            onClick={() => openNote(n.id)}
            onMouseMove={() => i !== selected && setSelected(i)}
            className={clsx("flex items-center gap-3 rounded-xl px-2.5 py-2 text-left", i === selected ? "bg-ink/10" : "hover:bg-ink/5")}
          >
            <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-ink/6 text-[15px]">
              {n.icon ?? <StickyNote className="size-4 text-fg-subtle" />}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13.5px]">{n.title || "Без названия"}</span>
              <span className="block truncate text-[12px] text-fg-subtle">{n.snippet}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

function Skeleton() {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-1">
      {Array.from({ length: 16 }, (_, i) => (
        <div key={i} className="flex animate-pulse flex-col items-center gap-2 px-1 pt-2.5 pb-2">
          <div className="size-10 rounded-xl bg-ink/8" />
          <div className="h-2 w-12 rounded-full bg-ink/6" />
        </div>
      ))}
    </div>
  );
}

export function AppsTab() {
  const query = usePanelStore((s) => s.query);
  const { data: apps, isPending, isError } = useApps();
  const byId = useAppsById();
  const pinnedIds = usePrefs((s) => s.pinned);
  const usage = usePrefs((s) => s.usage);

  const pinned = usePinnedEntries();
  const recent = useMemo(
    () =>
      Object.entries(usage)
        .sort(([, a], [, b]) => b.last - a.last)
        .map(([id]) => byId.get(id))
        .filter((a): a is AppEntry => !!a && !pinnedIds.includes(a.id))
        .slice(0, RECENT_COUNT),
    [usage, byId, pinnedIds],
  );

  if (spotifyTerm(query)) return <SpotifyResults />;
  if (query.trim()) return <SearchResults />;
  if (isPending) return <Skeleton />;
  if (isError || !apps) {
    return <EmptyState icon={SearchX} title="Не удалось загрузить приложения" text="Попробуйте открыть панель ещё раз." />;
  }

  return (
    <div className="flex flex-col gap-4 pb-2">
      <Section title="Закреплённые" action={<PinFromDisk />}>
        {pinned.length > 0 ? (
          <AppGrid apps={pinned} />
        ) : (
          <p className="px-1 text-[12px] text-fg-subtle">
            Правый клик по приложению → «Закрепить». Файлы и папки — кнопками справа или перетаскиванием на панель.
          </p>
        )}
      </Section>
      {recent.length > 0 && (
        <Section title="Недавние">
          <AppGrid apps={recent} />
        </Section>
      )}
      <Section title={`Все приложения · ${apps.length}`}>
        <AppGrid apps={apps} />
      </Section>
    </div>
  );
}
