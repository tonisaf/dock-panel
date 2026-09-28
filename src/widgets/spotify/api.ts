import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface SpotifyStatus {
  connected: boolean;
  user: string | null;
  error: string | null;
  /** False for sign-ins made before liking was added. */
  canLike: boolean;
  /** Likewise for adding tracks to playlists. */
  canEditPlaylists: boolean;
}

export interface Playlist {
  id: string;
  name: string;
  uri: string;
  image: string | null;
  tracks: number | null;
  owner: string | null;
  /** The user's own or a collaborative playlist. */
  editable: boolean;
}

export interface Library {
  likedTotal: number | null;
  likedUri: string;
  playlists: Playlist[];
}

export function useSpotifyStatus() {
  return useQuery({
    queryKey: ["spotify-status"],
    queryFn: () => invoke<SpotifyStatus>("spotify_status"),
    staleTime: 5 * 60_000,
  });
}

export function useLibrary(enabled: boolean) {
  return useQuery({
    queryKey: ["spotify-library"],
    queryFn: () => invoke<Library>("spotify_library"),
    enabled,
    staleTime: 10 * 60_000,
  });
}

/**
 * "played": started on a running Spotify; "launched": Spotify was closed, so
 * it was started and then told to play; "opened": only shown in the app.
 */
export const playUri = (uri: string, track?: string) =>
  invoke<"played" | "launched" | "opened">("spotify_play", { uri, track: track ?? null });

// ---- now playing ----------------------------------------------------------------

export interface CurrentTrack {
  id: string;
  uri: string;
  name: string;
  liked: boolean;
}

/** Spotify's own view of the current track; `trackKey` refetches it when the track changes. */
export function useSpotifyCurrent(trackKey: string | null, enabled: boolean) {
  return useQuery({
    queryKey: ["spotify-current", trackKey],
    queryFn: () => invoke<CurrentTrack | null>("spotify_current"),
    enabled: enabled && !!trackKey,
    // Liked or not: changes from here refresh it themselves.
    staleTime: 5 * 60_000,
  });
}

export const setLiked = (track: CurrentTrack, liked: boolean) =>
  invoke("spotify_set_liked", { id: track.id, uri: track.uri, liked });

// ---- devices --------------------------------------------------------------------

export interface Device {
  id: string;
  name: string;
  kind: string;
  active: boolean;
}

export function useDevices(enabled: boolean) {
  return useQuery({
    queryKey: ["spotify-devices"],
    queryFn: () => invoke<Device[]>("spotify_devices"),
    enabled,
    staleTime: 0,
  });
}

export const transferTo = (id: string) => invoke("spotify_transfer", { id });

// ---- search ---------------------------------------------------------------------

export interface SearchItem {
  kind: "track" | "artist" | "album" | "playlist";
  uri: string;
  name: string;
  subtitle: string;
  image: string | null;
  /** For tracks: the album, so playback continues past the track. */
  context: string | null;
}

/** "sp daft punk" → "daft punk"; also typed in the Russian layout ("ыз ..."). */
export function spotifyTerm(query: string) {
  return /^(?:sp|ыз)\s+(.+)$/i.exec(query.trimStart())?.[1].trim() || null;
}

export const searchSpotify = (query: string) => invoke<SearchItem[]>("spotify_search", { query });

export const queueTrack = (uri: string) => invoke("spotify_queue", { uri });

/** Plays a search result: tracks start within their album. */
export const playItem = (item: SearchItem) =>
  item.kind === "track" && item.context ? playUri(item.context, item.uri) : playUri(item.uri);

// ---- player ---------------------------------------------------------------------

export type Repeat = "off" | "context" | "track";

export interface PlayerState {
  playing: boolean;
  /** null when the device doesn't let its volume be changed. */
  volume: number | null;
  shuffle: boolean;
  repeat: Repeat;
  progressMs: number | null;
  durationMs: number | null;
}

const PLAYER = ["spotify-player"];

/** Volume, shuffle and repeat on the active device; refetched when the track changes. */
export function usePlayerState(trackKey: string | null, enabled: boolean) {
  return useQuery({
    queryKey: [...PLAYER, trackKey],
    queryFn: () => invoke<PlayerState | null>("spotify_player"),
    enabled: enabled && !!trackKey,
    staleTime: 5_000,
    refetchInterval: 15_000,
  });
}

/** Player calls that update the cached state at once and roll it back if Spotify refuses. */
export function usePlayerActions(trackKey: string | null) {
  const queryClient = useQueryClient();
  const key = [...PLAYER, trackKey];
  const optimistic = async (patch: Partial<PlayerState>, call: () => Promise<unknown>) => {
    const before = queryClient.getQueryData<PlayerState | null>(key);
    if (before) queryClient.setQueryData(key, { ...before, ...patch });
    try {
      await call();
    } catch (e) {
      queryClient.setQueryData(key, before);
      throw e;
    }
  };
  return {
    setVolume: (percent: number) =>
      optimistic({ volume: percent }, () => invoke("spotify_volume", { percent: Math.round(percent) })),
    setShuffle: (on: boolean) => optimistic({ shuffle: on }, () => invoke("spotify_shuffle", { on })),
    setRepeat: (mode: Repeat) => optimistic({ repeat: mode }, () => invoke("spotify_repeat", { mode })),
  };
}

export const seekSpotify = (positionMs: number) => invoke("spotify_seek", { positionMs: Math.round(positionMs) });

// ---- up next --------------------------------------------------------------------

export interface QueueItem {
  uri: string;
  name: string;
  subtitle: string;
  image: string | null;
  durationMs: number | null;
}

export function useUpNext(trackKey: string | null, enabled: boolean) {
  return useQuery({
    queryKey: ["spotify-up-next", trackKey],
    queryFn: () => invoke<QueueItem[]>("spotify_up_next"),
    enabled: enabled && !!trackKey,
    staleTime: 10_000,
  });
}

/** Jumps to the `count`-th upcoming track. */
export const skipAhead = (count: number) => invoke("spotify_skip", { count });

export const addToPlaylist = (playlistId: string, uri: string) =>
  invoke("spotify_add_to_playlist", { playlistId, uri });
