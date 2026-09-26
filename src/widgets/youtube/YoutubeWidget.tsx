import { useState } from "react";
import { Check, RefreshCw, SquarePlay, Undo2 } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import {
  ago,
  duration,
  startsAt,
  useYoutubeActions,
  useYoutubeFeed,
  useYoutubeSettings,
  views,
  type Video,
} from "./api";

const COLLAPSED = 5;
const EXPANDED = 20;
/** Unwatched videos younger than this get the "new" dot. */
const NEW_FOR_MS = 48 * 3600_000;

const badge = "absolute right-1 bottom-1 rounded px-1 text-[9.5px] leading-[15px] font-semibold text-white";

function Thumb({ video }: { video: Video }) {
  const [failed, setFailed] = useState(false);
  return (
    <div className="relative aspect-video w-24 shrink-0 overflow-hidden rounded-md bg-ink/8">
      {video.thumbnail && !failed && (
        <img
          src={video.thumbnail}
          loading="lazy"
          draggable={false}
          onError={() => setFailed(true)}
          // hqdefault is 4:3 with black bars; cropping to 16:9 hides them.
          className="size-full scale-[1.34] object-cover"
        />
      )}
      {video.live === "live" ? (
        <span className={clsx(badge, "bg-[#cc0000]")}>В ЭФИРЕ</span>
      ) : video.live === "upcoming" ? (
        <span className={clsx(badge, "bg-black/75")}>{video.short ? "Скоро" : "Премьера"}</span>
      ) : video.short ? (
        <span className={clsx(badge, "bg-black/75")}>Shorts</span>
      ) : (
        video.duration != null && <span className={clsx(badge, "bg-black/75 tabular-nums")}>{duration(video.duration)}</span>
      )}
    </div>
  );
}

function Row({ video }: { video: Video }) {
  const { open, setWatched } = useYoutubeActions();
  const fresh = !video.watched && Date.now() - video.published < NEW_FOR_MS;
  const when =
    video.live === "upcoming" && video.starts
      ? `начнётся ${startsAt(video.starts)}`
      : video.live === "live"
        ? "идёт сейчас"
        : ago(video.published);
  const meta = [
    video.channelTitle,
    when,
    video.views != null && !video.live ? `${views(video.views)} просмотров` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className={clsx("group relative flex gap-2.5 rounded-lg p-1 transition-colors hover:bg-ink/5", video.watched && "opacity-55")}>
      <button onClick={() => open(video)} title={video.title} className="flex min-w-0 flex-1 gap-2.5 text-left">
        <Thumb video={video} />
        <div className="min-w-0 flex-1 py-0.5">
          <div className="line-clamp-2 text-[12.5px] leading-snug">
            {fresh && <span className="mr-1.5 inline-block size-1.5 -translate-y-0.5 rounded-full bg-accent" />}
            {video.title}
          </div>
          <div className="mt-0.5 truncate text-[11px] text-fg-subtle">{meta}</div>
        </div>
      </button>
      <button
        onClick={() => setWatched(video, !video.watched)}
        title={video.watched ? "Отметить непросмотренным" : "Отметить просмотренным"}
        className="absolute top-1 right-1 hidden size-6 place-items-center rounded-md bg-popover text-fg-muted shadow group-hover:grid hover:text-fg"
      >
        {video.watched ? <Undo2 className="size-3.5" /> : <Check className="size-3.5" />}
      </button>
    </div>
  );
}

export function YoutubeWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const { data: settings } = useYoutubeSettings();
  const hasChannels = (settings?.channels.length ?? 0) > 0;
  const { data: feed, isPending, isError, error } = useYoutubeFeed(hasChannels);
  const { refresh } = useYoutubeActions();
  const [refreshing, setRefreshing] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const reload = () => {
    setRefreshing(true);
    refresh()
      .catch(console.error)
      .finally(() => setRefreshing(false));
  };

  const action = hasChannels && (
    <button
      onClick={reload}
      disabled={refreshing}
      title="Обновить"
      className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
    >
      <RefreshCw className={clsx("size-3.5", refreshing && "animate-spin")} />
    </button>
  );

  if (!hasChannels) {
    return (
      <Card title="YouTube" icon={SquarePlay}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Добавьте каналы в настройках: ссылкой или импортом подписок →
        </button>
      </Card>
    );
  }

  const videos = feed?.videos ?? [];
  const shown = videos.slice(0, expanded ? EXPANDED : COLLAPSED);
  const allFailed = feed && feed.failed > 0 && feed.failed === feed.channels;

  return (
    <Card title="YouTube" icon={SquarePlay} action={action} className="hover:bg-surface">
      {isPending ? (
        <p className="text-[12px] text-fg-subtle">Загружаю ленты каналов…</p>
      ) : isError ? (
        <p className="text-[12px] text-warn">{String(error)}</p>
      ) : (
        <div className="-mx-1 flex flex-col gap-0.5">
          {feed && feed.failed > 0 && (
            <p className="px-1 pb-1 text-[11.5px] text-warn">
              {allFailed
                ? `YouTube не отвечает${videos.length ? ", показываю сохранённое" : ""}. Может помочь VPN.`
                : `Не обновились каналов: ${feed.failed} из ${feed.channels}`}
            </p>
          )}
          {shown.length === 0 ? (
            <p className="px-1 text-[12px] text-fg-subtle">Пока нет видео</p>
          ) : (
            shown.map((v) => <Row key={v.id} video={v} />)
          )}
          {videos.length > COLLAPSED && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="mt-1 self-start px-1 text-[12px] text-fg-subtle hover:text-fg"
            >
              {expanded ? "Свернуть" : "Ещё видео"}
            </button>
          )}
        </div>
      )}
    </Card>
  );
}
