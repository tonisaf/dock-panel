import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface Channel {
  id: string;
  title: string;
}

export interface YoutubeSettings {
  channels: Channel[];
  notify: boolean;
  hideShorts: boolean;
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
    staleTime: 60_000,
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

    open: (v: Video) => {
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

    setOptions: async (options: { notify?: boolean; hideShorts?: boolean }) => {
      await invoke("youtube_set_options", { notify: options.notify ?? null, hideShorts: options.hideShorts ?? null });
      reload();
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
