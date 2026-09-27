import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface DiscordMember {
  id: string;
  name: string;
  avatar: string;
  speaking: boolean;
  muted: boolean;
  deafened: boolean;
}

export interface DiscordChannel {
  id: string;
  guildId: string;
  name: string;
  guildName: string;
  /** Empty until Discord has been read (or when it doesn't share the channel). */
  members: DiscordMember[];
}

export interface DiscordVoice {
  mute: boolean;
  deaf: boolean;
  /** 0–100. */
  inputVolume: number;
  mode: "VOICE_ACTIVITY" | "PUSH_TO_TALK";
  inputDevice: string;
  inputDevices: { id: string; name: string }[];
}

export interface DiscordState {
  configured: boolean;
  /** Discord is running and the panel is signed in to it. */
  connected: boolean;
  error: string | null;
  user: DiscordMember | null;
  voice: DiscordVoice | null;
  current: DiscordChannel | null;
  watched: DiscordChannel[];
  notify: boolean;
  muteShortcut: string | null;
}

export type WatchedChannel = Omit<DiscordChannel, "members">;

export interface VoiceChange {
  mute?: boolean;
  deaf?: boolean;
  inputVolume?: number;
  mode?: DiscordVoice["mode"];
  inputDevice?: string;
}

const KEY = ["discord"];

/** The backend pushes every change (joins, speaking, mute), so there is no polling. */
export function useDiscord() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const un = listen<DiscordState>("discord:changed", (e) => queryClient.setQueryData(KEY, e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [queryClient]);
  return useQuery({ queryKey: KEY, queryFn: () => invoke<DiscordState>("discord_state"), staleTime: Infinity });
}

export function useDiscordActions() {
  const queryClient = useQueryClient();
  const put = (s: DiscordState) => queryClient.setQueryData(KEY, s);
  return {
    login: async (clientId: string, clientSecret: string) => put(await invoke<DiscordState>("discord_login", { clientId, clientSecret })),
    logout: async () => put(await invoke<DiscordState>("discord_logout")),
    setWatched: async (channels: WatchedChannel[]) => put(await invoke<DiscordState>("discord_set_watched", { channels })),
    setNotify: async (on: boolean) => put(await invoke<DiscordState>("discord_set_notify", { on })),
    setMuteShortcut: async (shortcut: string | null) => put(await invoke<DiscordState>("discord_set_mute_shortcut", { shortcut })),
    join: (channelId: string | null) => invoke("discord_join", { channelId }),

    /** Shows the change at once; Discord's answer replaces it. */
    voice: async (change: VoiceChange) => {
      const before = queryClient.getQueryData<DiscordState>(KEY);
      if (before?.voice) put({ ...before, voice: { ...before.voice, ...change } });
      try {
        put(await invoke<DiscordState>("discord_voice", { change }));
      } catch (e) {
        if (before) put(before);
        throw e;
      }
    },
  };
}

export interface Guild {
  id: string;
  name: string;
  icon: string | null;
}

export function useGuilds(enabled: boolean) {
  return useQuery({ queryKey: ["discord-guilds"], queryFn: () => invoke<Guild[]>("discord_guilds"), enabled, staleTime: 60_000 });
}

export function useVoiceChannels(guildId: string | null) {
  return useQuery({
    queryKey: ["discord-channels", guildId],
    queryFn: () => invoke<WatchedChannel[]>("discord_channels", { guildId }),
    enabled: guildId != null,
    staleTime: 60_000,
  });
}
