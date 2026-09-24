import { useEffect, useMemo, useRef, type ReactNode } from "react";
import { SearchX } from "lucide-react";
import { useApps, useAppsById, useSearchResults, type AppEntry } from "../lib/apps";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { AppGrid, AppRow } from "../components/AppTile";
import { EmptyState } from "../components/Card";

const RECENT_COUNT = 8;

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="px-1 text-[12px] font-medium text-fg-subtle">{title}</h3>
      {children}
    </section>
  );
}

function SearchResults() {
  const results = useSearchResults();
  const { selected, setSelected } = usePanelStore();
  const rows = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    rows.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (results.length === 0) {
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

  const pinned = useMemo(
    () => pinnedIds.map((id) => byId.get(id)).filter((a): a is AppEntry => !!a),
    [pinnedIds, byId],
  );
  const recent = useMemo(
    () =>
      Object.entries(usage)
        .sort(([, a], [, b]) => b.last - a.last)
        .map(([id]) => byId.get(id))
        .filter((a): a is AppEntry => !!a && !pinnedIds.includes(a.id))
        .slice(0, RECENT_COUNT),
    [usage, byId, pinnedIds],
  );

  if (query.trim()) return <SearchResults />;
  if (isPending) return <Skeleton />;
  if (isError || !apps) {
    return <EmptyState icon={SearchX} title="Не удалось загрузить приложения" text="Попробуйте открыть панель ещё раз." />;
  }

  return (
    <div className="flex flex-col gap-4 pb-2">
      {pinned.length > 0 && (
        <Section title="Закреплённые">
          <AppGrid apps={pinned} />
        </Section>
      )}
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
