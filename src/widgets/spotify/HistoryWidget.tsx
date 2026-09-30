import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Disc3, Loader2, Play } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import {
  playItem,
  useListening,
  useRecent,
  useSpotifyStatus,
  useTop,
  type ListeningSummary,
  type SearchItem,
  type TopKind,
  type TopRange,
} from "./api";
import { Cover } from "./Cover";
import { formatListened, peakHour, shortDay } from "./format";

type Tab = "recent" | "top" | "stats";

const TABS: { id: Tab; label: string }[] = [
  { id: "recent", label: "Недавно" },
  { id: "top", label: "Топ" },
  { id: "stats", label: "Статистика" },
];
const KINDS: { id: TopKind; label: string }[] = [
  { id: "tracks", label: "Треки" },
  { id: "artists", label: "Исполнители" },
];
const RANGES: { id: TopRange; label: string }[] = [
  { id: "short_term", label: "4 недели" },
  { id: "medium_term", label: "6 месяцев" },
  { id: "long_term", label: "Всё время" },
];
const STAT_RANGES = [
  { id: 7, label: "7 дней" },
  { id: 30, label: "30 дней" },
  { id: 90, label: "90 дней" },
];
const COLLAPSED = 6;

function Chips<T extends string | number>({
  items,
  value,
  onChange,
}: {
  items: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((i) => (
        <button
          key={i.id}
          onClick={() => onChange(i.id)}
          className={clsx(
            "rounded-full px-2.5 py-0.5 text-[11.5px] transition-colors",
            i.id === value ? "bg-ink/15 text-fg" : "text-fg-subtle hover:bg-ink/8 hover:text-fg",
          )}
        >
          {i.label}
        </button>
      ))}
    </div>
  );
}

function Row({
  item,
  rank,
  busy,
  onPlay,
}: {
  item: SearchItem;
  rank?: number;
  busy: boolean;
  onPlay: () => void;
}) {
  return (
    <button
      onClick={onPlay}
      title={`${item.name} · ${item.subtitle}`}
      className="group flex w-full items-center gap-2.5 rounded-lg px-1.5 py-1 text-left outline-none hover:bg-ink/8 focus-visible:bg-ink/8"
    >
      {rank != null && <span className="w-4 shrink-0 text-right text-[11px] tabular-nums text-fg-subtle">{rank}</span>}
      <div className={clsx("relative size-9 shrink-0 overflow-hidden bg-ink/8", item.kind === "artist" ? "rounded-full" : "rounded-md")}>
        <Cover src={item.image} />
        <div className="absolute inset-0 grid place-items-center bg-black/45 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
          {busy ? <Loader2 className="size-4 animate-spin text-white" /> : <Play className="size-4 text-white" fill="currentColor" />}
        </div>
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[12.5px] leading-tight">{item.name}</div>
        <div className="truncate text-[11px] text-fg-subtle">{item.subtitle}</div>
      </div>
    </button>
  );
}

/** Rows that start playing on click; the panel steps aside if Spotify had to be opened. */
function PlayList({ items, ranked }: { items: SearchItem[]; ranked?: boolean }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const shown = expanded ? items : items.slice(0, COLLAPSED);

  const play = async (item: SearchItem) => {
    setBusy(item.uri);
    try {
      const outcome = await playItem(item);
      if (outcome !== "played") usePanelStore.getState().setOpen(false);
      setTimeout(() => queryClient.invalidateQueries({ queryKey: ["media"] }), 800);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(null);
    }
  };

  if (items.length === 0) return <p className="text-[12px] text-fg-subtle">Пока пусто</p>;
  return (
    <>
      <div className="-mx-1.5 flex flex-col">
        {shown.map((item, i) => (
          <Row key={item.uri} item={item} rank={ranked ? i + 1 : undefined} busy={busy === item.uri} onPlay={() => play(item)} />
        ))}
      </div>
      {items.length > COLLAPSED && (
        <button
          onClick={() => setExpanded(!expanded)}
          className="mt-1.5 w-full rounded-lg py-1 text-[12px] text-fg-subtle hover:bg-ink/6 hover:text-fg"
        >
          {expanded ? "Свернуть" : `Ещё ${items.length - COLLAPSED}`}
        </button>
      )}
    </>
  );
}

function Recent({ enabled }: { enabled: boolean }) {
  const { data, isPending, isError, error } = useRecent(enabled);
  if (isError) return <p className="text-[12px] leading-relaxed text-warn">{String(error)}</p>;
  if (isPending) return <p className="text-[12px] text-fg-subtle">Загрузка…</p>;
  return <PlayList items={data} />;
}

function Top({ enabled }: { enabled: boolean }) {
  const [kind, setKind] = useState<TopKind>("tracks");
  const [range, setRange] = useState<TopRange>("short_term");
  const { data, isPending, isError, error } = useTop(kind, range, enabled);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <Chips items={KINDS} value={kind} onChange={setKind} />
        <Chips items={RANGES} value={range} onChange={setRange} />
      </div>
      {isError ? (
        <p className="text-[12px] leading-relaxed text-warn">{String(error)}</p>
      ) : isPending ? (
        <p className="text-[12px] text-fg-subtle">Загрузка…</p>
      ) : (
        <PlayList items={data} ranked />
      )}
    </div>
  );
}

function Bars({
  values,
  labels,
  highlight,
  className,
}: {
  values: number[];
  labels: string[];
  highlight?: number | null;
  className: string;
}) {
  const max = Math.max(...values, 1);
  return (
    <div className={clsx("flex items-end gap-[3px]", className)}>
      {values.map((v, i) => (
        <div key={i} title={labels[i]} className="flex h-full min-w-0 flex-1 items-end">
          <div
            className={clsx("w-full rounded-[2px]", i === highlight ? "bg-accent" : "bg-fg/25")}
            // A sliver for empty slots keeps the axis readable.
            style={{ height: v > 0 ? `${Math.max(6, (v / max) * 100)}%` : "2px", opacity: v > 0 ? 1 : 0.4 }}
          />
        </div>
      ))}
    </div>
  );
}

function Figure({ value, caption }: { value: string; caption: string }) {
  return (
    <div className="min-w-0 flex-1">
      <div className="truncate text-[17px] font-semibold leading-tight tabular-nums">{value}</div>
      <div className="truncate text-[11px] text-fg-subtle">{caption}</div>
    </div>
  );
}

function StatsBody({ s }: { s: ListeningSummary }) {
  const peak = peakHour(s.hours);
  const hourLabels = s.hours.map((ms, h) => `${String(h).padStart(2, "0")}:00 · ${formatListened(ms)}`);
  const dayLabels = s.daily.map((d) => `${shortDay(d.day)} · ${formatListened(d.ms)}`);
  const today = s.daily.length - 1;
  return (
    <div className="flex flex-col gap-3.5">
      <div className="flex gap-3">
        <Figure value={formatListened(s.totalMs)} caption="прослушано" />
        <Figure value={String(s.plays)} caption="прослушиваний" />
        <Figure value={String(s.activeDays)} caption="дней с музыкой" />
      </div>

      <div>
        <Bars values={s.daily.map((d) => d.ms)} labels={dayLabels} highlight={today} className="h-12" />
        <div className="mt-1 flex justify-between text-[10px] text-fg-subtle">
          <span>{shortDay(s.daily[0]?.day ?? "")}</span>
          <span>сегодня</span>
        </div>
      </div>

      <div>
        <Bars values={s.hours} labels={hourLabels} highlight={peak} className="h-8" />
        <div className="mt-1 flex justify-between text-[10px] text-fg-subtle">
          <span>0:00</span>
          <span>{peak == null ? "" : `чаще всего около ${peak}:00`}</span>
          <span>24:00</span>
        </div>
      </div>

      {s.artists.length > 0 && (
        <div>
          <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Исполнители</div>
          {s.artists.map((a, i) => (
            <div key={a.name} className="flex items-baseline gap-2 py-0.5 text-[12.5px]">
              <span className="w-4 shrink-0 text-right text-[11px] tabular-nums text-fg-subtle">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate">{a.name}</span>
              <span className="shrink-0 text-[11.5px] tabular-nums text-fg-subtle">{formatListened(a.ms)}</span>
            </div>
          ))}
        </div>
      )}

      {s.tracks.length > 0 && (
        <div>
          <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Треки</div>
          {s.tracks.map((t, i) => (
            <div key={`${t.artist}|${t.title}`} className="flex items-baseline gap-2 py-0.5 text-[12.5px]" title={`${t.artist} — ${t.title}`}>
              <span className="w-4 shrink-0 text-right text-[11px] tabular-nums text-fg-subtle">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate">
                {t.title} <span className="text-fg-subtle">· {t.artist}</span>
              </span>
              <span className="shrink-0 text-[11.5px] tabular-nums text-fg-subtle">{t.plays}×</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Stats() {
  const [days, setDays] = useState(7);
  const { data } = useListening(days);
  return (
    <div className="flex flex-col gap-3">
      <Chips items={STAT_RANGES} value={days} onChange={setDays} />
      {!data ? (
        <p className="text-[12px] text-fg-subtle">Загрузка…</p>
      ) : data.totalMs === 0 ? (
        <p className="text-[12px] leading-relaxed text-fg-subtle">
          Пока пусто. Панель считает время, пока играет Spotify на этом компьютере, и хранит цифры только у вас.
        </p>
      ) : (
        <StatsBody s={data} />
      )}
    </div>
  );
}

export function HistoryWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const { data: status } = useSpotifyStatus();
  const connected = !!status?.connected && !status.error;
  const [tab, setLocalTab] = useState<Tab>("recent");

  // The statistics are local; the other two tabs need Spotify's history.
  const needs =
    tab === "stats" ? null : !connected ? "Войдите в Spotify в настройках →" : !status?.canHistory ? "Чтобы видеть историю, выйдите из Spotify в настройках и войдите снова →" : null;

  return (
    <Card title="Моя музыка" icon={Disc3}>
      <div className="mb-3 -mt-0.5">
        <Chips items={TABS} value={tab} onChange={setLocalTab} />
      </div>
      {needs ? (
        <button onClick={() => setTab("settings")} className="text-left text-[12px] leading-relaxed text-fg-subtle hover:text-fg">
          {needs}
        </button>
      ) : tab === "recent" ? (
        <Recent enabled={connected} />
      ) : tab === "top" ? (
        <Top enabled={connected} />
      ) : (
        <Stats />
      )}
    </Card>
  );
}
