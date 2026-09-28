import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface Channel {
  id: string;
  title: string;
  /** From the Google account's subscriptions. */
  google: boolean;
}

export interface YoutubeSettings {
  channels: Channel[];
  notify: boolean;
  hideShorts: boolean;
  syncGoogle: boolean;
  /** Why the last subscriptions sync failed. */
  googleError: string | null;
}

export interface Video {
  id: string;
  channelId: string;
  channelTitle: string;
  title: string;
  /** Unix ms. */
  published: number;
  thumbnail: string | null;
  views: number | null;
  short: boolean;
  url: string;
  watched: boolean;
  /** Seconds (with the Google sign-in); null for streams and premieres. */
  duration: number | null;
  /** A stream on air, or a scheduled stream or premiere. */
  live: "live" | "upcoming" | null;
  /** Scheduled start of an upcoming one, Unix ms. */
  starts: number | null;
}

export interface FeedView {
  videos: Video[];
  /** Unix ms of the last refresh; 0 before the first. */
  refreshed: number;
  /** Channels that failed in the last refresh; their older videos still show. */
  failed: number;
  channels: number;
}

const SETTINGS = ["youtube-settings"];
const FEED = ["youtube-feed"];

export function useYoutubeSettings() {
  return useQuery({ queryKey: SETTINGS, queryFn: () => invoke<YoutubeSettings>("youtube_settings"), staleTime: Infinity });
}

export function useYoutubeFeed(enabled: boolean) {
  return useQuery({
    queryKey: FEED,
    queryFn: () => invoke<FeedView>("youtube_feed", { force: false }),
    enabled,
    // Rust refreshes the feeds every 15 min and says so (youtube:changed).
    staleTime: 15 * 60_000,
  });
}

export function invalidateYoutube(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: FEED });
  queryClient.invalidateQueries({ queryKey: SETTINGS });
}

export function useYoutubeActions() {
  const queryClient = useQueryClient();
  const setWatchedLocally = (id: string, watched: boolean) =>
    queryClient.setQueryData<FeedView>(FEED, (f) => f && { ...f, videos: f.videos.map((v) => (v.id === id ? { ...v, watched } : v)) });
  const reload = () => invalidateYoutube(queryClient);

  return {
    refresh: async () => queryClient.setQueryData(FEED, await invoke<FeedView>("youtube_feed", { force: true })),

    /**
     * Plays in the panel's mpv window (no ads; marked watched at 90 %), or
     * opens the browser when mpv or yt-dlp isn't installed.
     */
    open: (v: Video, start?: number) => {
      invoke("player_play", { id: v.id, url: v.url, title: v.title, channel: v.channelTitle, start: start ?? null }).catch(
        () => {
          setWatchedLocally(v.id, true);
          invoke("youtube_set_watched", { id: v.id, watched: true }).catch(console.error);
          openUrl(v.url).catch(console.error);
        },
      );
    },

    /** The video on YouTube's site, e.g. for comments or the account's history. */
    openInBrowser: (v: Video) => {
      setWatchedLocally(v.id, true);
      invoke("youtube_set_watched", { id: v.id, watched: true }).catch(console.error);
      openUrl(v.url).catch(console.error);
    },

    setWatched: (v: Video, watched: boolean) => {
      setWatchedLocally(v.id, watched);
      invoke("youtube_set_watched", { id: v.id, watched }).catch(console.error);
    },

    add: async (input: string) => {
      const channel = await invoke<Channel>("youtube_add", { input });
      reload();
      return channel;
    },

    /** Number of channels added; 0 if the dialog was cancelled or all were known. */
    importTakeout: async () => {
      const count = await invoke<number>("youtube_import");
      reload();
      return count;
    },

    remove: async (id: string) => {
      await invoke("youtube_remove", { id });
      reload();
    },

    setOptions: async (options: { notify?: boolean; hideShorts?: boolean; syncGoogle?: boolean }) => {
      try {
        await invoke("youtube_set_options", {
          notify: options.notify ?? null,
          hideShorts: options.hideShorts ?? null,
          syncGoogle: options.syncGoogle ?? null,
        });
      } finally {
        reload();
      }
    },
  };
}

/** "5 мин", "3 ч", "вчера", "4 дн.", then a date. */
export function ago(ms: number, now = Date.now()) {
  const min = Math.max(0, Math.round((now - ms) / 60_000));
  if (min < 60) return `${Math.max(1, min)} мин`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h} ч`;
  const d = Math.round(h / 24);
  if (d === 1) return "вчера";
  if (d < 7) return `${d} дн.`;
  return new Date(ms).toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
}

/** 1 234 → "1,2 тыс.", 3 400 000 → "3,4 млн". */
export function views(n: number) {
  const fmt = (x: number) => x.toLocaleString("ru-RU", { maximumFractionDigits: x < 10 ? 1 : 0 });
  if (n >= 1_000_000) return `${fmt(n / 1_000_000)} млн`;
  if (n >= 1_000) return `${fmt(n / 1_000)} тыс.`;
  return String(n);
}

/** 754 → "12:34", 3723 → "1:02:03". */
export function duration(seconds: number) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const sec = String(seconds % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** "сегодня в 18:00", "завтра в 9:30", "3 окт. в 20:00". */
export function startsAt(ms: number, now = new Date()) {
  const d = new Date(ms);
  const time = d.toLocaleTimeString("ru-RU", { hour: "numeric", minute: "2-digit" });
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(d) - day(now)) / 86_400_000);
  if (diff === 0) return `сегодня в ${time}`;
  if (diff === 1) return `завтра в ${time}`;
  return `${d.toLocaleDateString("ru-RU", { day: "numeric", month: "short" })} в ${time}`;
}
