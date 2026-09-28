import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Calculator, ChevronDown, Folder, Globe, SearchX, StickyNote } from "lucide-react";
import clsx from "clsx";
import { useApps, useFrequentApps, useHiddenApps, usePinnedEntries, useVisibleApps } from "../lib/apps";
import { activateItem, ENGINES, useSearchItems, type SearchItem } from "../lib/search";
import { usePanelStore } from "../store";
import { AppGrid, AppRow } from "../components/AppTile";
import { EmptyState } from "../components/Card";
import { Collapse } from "../components/Collapse";
import { PinFromDisk } from "../components/PinFromDisk";
import { PinnedGrid } from "../components/PinnedGrid";
import { spotifyTerm } from "../widgets/spotify/api";
import { SpotifyResults } from "../widgets/spotify/SpotifySearch";

const FREQUENT_COUNT = 8;

function Section({ title, action, children }: { title: ReactNode; action?: ReactNode; children: ReactNode }) {
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

const SECTION_TITLE: Partial<Record<SearchItem["kind"], string>> = { note: "Заметки", file: "Файлы" };

function Row({ item, active, onHover, rowRef }: { item: SearchItem; active: boolean; onHover: () => void; rowRef: (el: HTMLButtonElement | null) => void }) {
  const activate = () => activateItem(item);
  const base = clsx("flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left outline-none transition-colors", active ? "bg-ink/10" : "hover:bg-surface");
  const enter = active && <kbd className="shrink-0 rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-subtle">Enter</kbd>;
  const badge = (content: ReactNode) => <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-ink/6 text-[15px]">{content}</span>;

  switch (item.kind) {
    case "app":
      return <AppRow ref={rowRef} app={item.app} active={active} onHover={onHover} onClick={activate} />;
    case "file":
      return <AppRow ref={rowRef} app={item.app} active={active} onHover={onHover} onClick={activate} detail={item.file.folder} />;
    case "answer":
      return (
        <button ref={rowRef} onClick={activate} onMouseMove={onHover} className={base}>
          {badge(<Calculator className="size-4 text-accent" />)}
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[17px] font-semibold tabular-nums">{item.answer.text}</span>
            <span className="block truncate text-[11.5px] text-fg-subtle">{item.answer.detail}</span>
          </span>
          {enter}
        </button>
      );
    case "note":
      return (
        <button ref={rowRef} onClick={activate} onMouseMove={onHover} className={base}>
          {badge(item.note.icon ?? <StickyNote className="size-4 text-fg-subtle" />)}
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13.5px]">{item.note.title || "Без названия"}</span>
            <span className="block truncate text-[12px] text-fg-subtle">{item.note.snippet}</span>
          </span>
          {enter}
        </button>
      );
    case "web":
      return (
        <button ref={rowRef} onClick={activate} onMouseMove={onHover} className={base}>
          {badge(<Globe className="size-4 text-fg-subtle" />)}
          <span className="min-w-0 flex-1 truncate text-[13.5px]">
            Искать «{item.query}» в {ENGINES[item.engine].label}
          </span>
          {enter}
        </button>
      );
  }
}

function SearchResults() {
  const items = useSearchItems();
  const { selected, setSelected } = usePanelStore();
  const rows = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    rows.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  // Only the web row: nothing found locally.
  const nothing = items.length <= 1;
  return (
    <div className="flex flex-col gap-0.5 pb-2">
      {nothing && <EmptyState icon={SearchX} title="Ничего не найдено" text="Попробуйте другое название, первые буквы слов — или поищите в интернете." />}
      {items.map((item, i) => {
        const title = SECTION_TITLE[item.kind];
        const first = title && items[i - 1]?.kind !== item.kind;
        return (
          <div key={`${item.kind}-${i}`} className={clsx(item.kind === "web" && !nothing && "mt-1 border-t border-stroke pt-1")}>
            {first && <h3 className="px-1 pt-2 pb-1 text-[12px] font-medium text-fg-subtle">{title}</h3>}
            <Row
              item={item}
              active={i === selected}
              onHover={() => i !== selected && setSelected(i)}
              rowRef={(el) => {
                rows.current[i] = el;
              }}
            />
          </div>
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

/** First letters of the list, each jumping to its first app. */
function Letters({ names, onJump }: { names: string[]; onJump: (letter: string) => void }) {
  const letters = useMemo(() => [...new Set(names.map((n) => n[0]?.toUpperCase()).filter(Boolean))], [names]);
  if (letters.length < 6) return null;
  return (
    <div className="flex flex-wrap gap-0.5 px-0.5 pb-1">
      {letters.map((l) => (
        <button
          key={l}
          onClick={() => onJump(l)}
          className="grid h-6 min-w-6 place-items-center rounded-md px-1 text-[11.5px] text-fg-subtle hover:bg-ink/10 hover:text-fg"
        >
          {l}
        </button>
      ))}
    </div>
  );
}

function HiddenApps() {
  const hidden = useHiddenApps();
  const [open, setOpen] = useState(false);
  if (hidden.length === 0) return null;
  return (
    <section className="flex flex-col gap-1">
      <button onClick={() => setOpen(!open)} className="flex items-center gap-1 px-1 text-left text-[12px] font-medium text-fg-subtle hover:text-fg">
        Скрытые · {hidden.length}
        <ChevronDown className={clsx("size-3.5 transition-transform", open && "rotate-180")} />
      </button>
      <Collapse open={open} className="-mx-2 px-2">
        <p className="px-1 pb-1 text-[11.5px] text-fg-subtle">Правый клик → «Показывать в списке», чтобы вернуть.</p>
        <AppGrid apps={hidden} />
      </Collapse>
    </section>
  );
}

export function AppsTab() {
  const query = usePanelStore((s) => s.query);
  const { isPending, isError } = useApps();
  const apps = useVisibleApps();
  const pinned = usePinnedEntries();
  const frequent = useFrequentApps(FREQUENT_COUNT + 8).filter((a) => !pinned.some((p) => p.id === a.id || p.folder?.items.some((i) => i.id === a.id)));
  const allRef = useRef<HTMLDivElement>(null);

  if (spotifyTerm(query)) return <SpotifyResults />;
  if (query.trim()) return <SearchResults />;
  if (isPending) return <Skeleton />;
  if (isError) {
    return <EmptyState icon={SearchX} title="Не удалось загрузить приложения" text="Попробуйте открыть панель ещё раз." />;
  }

  const jump = (letter: string) => {
    const app = apps.find((a) => a.name[0]?.toUpperCase() === letter);
    allRef.current?.querySelector(`[title="${CSS.escape(app?.name ?? "")}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  return (
    <div className="flex flex-col gap-4 pb-2">
      <Section title="Закреплённые" action={<PinFromDisk />}>
        {pinned.length > 0 ? (
          <>
            <PinnedGrid entries={pinned} />
            <p className="flex items-center gap-1 px-1 text-[11px] text-fg-subtle">
              <Folder className="size-3" /> Перетаскивайте, чтобы переставить; на другую иконку — чтобы собрать папку
            </p>
          </>
        ) : (
          <p className="px-1 text-[12px] text-fg-subtle">
            Правый клик по приложению → «Закрепить». Файлы и папки — кнопками справа или перетаскиванием на панель.
          </p>
        )}
      </Section>
      {frequent.length > 0 && (
        <Section title="Часто используемые">
          <AppGrid apps={frequent.slice(0, FREQUENT_COUNT)} />
        </Section>
      )}
      <Section title={`Все приложения · ${apps.length}`}>
        <Letters names={apps.map((a) => a.name)} onJump={jump} />
        <div ref={allRef}>
          <AppGrid apps={apps} />
        </div>
      </Section>
      <HiddenApps />
    </div>
  );
}
