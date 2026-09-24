import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface SpotifyStatus {
  connected: boolean;
  user: string | null;
  error: string | null;
  /** False for sign-ins made before liking was added. */
  canLike: boolean;
}

export interface Playlist {
  id: string;
  name: string;
  uri: string;
  image: string | null;
  tracks: number | null;
  owner: string | null;
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
    staleTime: 60_000,
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
