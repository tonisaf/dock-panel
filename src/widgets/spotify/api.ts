import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface SpotifyStatus {
  connected: boolean;
  user: string | null;
  error: string | null;
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

/** "played" when playback started via the API, "opened" when handed to the app. */
export const playUri = (uri: string) => invoke<"played" | "opened">("spotify_play", { uri });
