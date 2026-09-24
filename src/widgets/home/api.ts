import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface LampState {
  power: boolean;
  /** 1–100. */
  bright: number;
  /** Kelvin. */
  ct: number;
  /** 0xRRGGBB. */
  rgb: number;
  /** 1 = RGB, 2 = colour temperature, 3 = HSV. */
  colorMode: number;
}

export interface Lamp {
  id: string;
  name: string;
  model: string;
  supportsCt: boolean;
  supportsRgb: boolean;
  ctMin: number;
  ctMax: number;
  /** null when the lamp didn't answer. */
  state: LampState | null;
}

export interface SpeakerState {
  /** 0–1. */
  volume: number;
  muted: boolean;
  app: string | null;
  title: string | null;
  artist: string | null;
  image: string | null;
  playerState: "PLAYING" | "PAUSED" | "BUFFERING" | "IDLE" | null;
  canNext: boolean;
  canPrev: boolean;
}

export interface Speaker {
  id: string;
  name: string;
  model: string;
  state: SpeakerState | null;
}

export interface HomeState {
  lamps: Lamp[];
  speakers: Speaker[];
}

export interface LampChange {
  power?: boolean;
  bright?: number;
  ct?: number;
  rgb?: number;
}

export type SpeakerControl =
  | { action: "play" | "pause" | "next" | "prev" }
  | { action: "volume"; level: number }
  | { action: "mute"; muted: boolean };

const KEY = ["home"];

export function useHome() {
  return useQuery({
    queryKey: KEY,
    queryFn: () => invoke<HomeState>("home_state", { rescan: false }),
    refetchInterval: 10_000,
    staleTime: 5_000,
  });
}

/** Actions update the cache at once and replace it with the device's real state when it answers. */
export function useHomeActions() {
  const queryClient = useQueryClient();
  const patch = (fn: (s: HomeState) => HomeState) => queryClient.setQueryData<HomeState>(KEY, (s) => s && fn(s));
  const putLamp = (lamp: Lamp) => patch((s) => ({ ...s, lamps: s.lamps.map((l) => (l.id === lamp.id ? lamp : l)) }));
  const putSpeaker = (sp: Speaker) =>
    patch((s) => ({ ...s, speakers: s.speakers.map((x) => (x.id === sp.id ? sp : x)) }));

  return {
    rescan: async () => queryClient.setQueryData(KEY, await invoke<HomeState>("home_state", { rescan: true })),

    setLamp: async (lamp: Lamp, change: LampChange) => {
      if (lamp.state) {
        const optimistic = { ...lamp.state, ...change, power: change.power ?? true };
        if (change.rgb != null) optimistic.colorMode = 1;
        if (change.ct != null) optimistic.colorMode = 2;
        putLamp({ ...lamp, state: optimistic });
      }
      try {
        putLamp(await invoke<Lamp>("home_lamp_set", { id: lamp.id, change }));
      } catch (e) {
        putLamp(lamp);
        throw e;
      }
    },

    controlSpeaker: async (sp: Speaker, control: SpeakerControl) => {
      if (sp.state) {
        const next = { ...sp.state };
        if (control.action === "volume") next.volume = control.level;
        if (control.action === "mute") next.muted = control.muted;
        if (control.action === "play") next.playerState = "PLAYING";
        if (control.action === "pause") next.playerState = "PAUSED";
        putSpeaker({ ...sp, state: next });
      }
      try {
        putSpeaker(await invoke<Speaker>("home_speaker_control", { id: sp.id, control }));
      } catch (e) {
        putSpeaker(sp);
        throw e;
      }
    },

    rename: async (id: string, name: string) => {
      await invoke("home_rename", { id, name });
      await queryClient.invalidateQueries({ queryKey: KEY });
    },

    forget: async (id: string) => {
      await invoke("home_forget", { id });
      await queryClient.invalidateQueries({ queryKey: KEY });
    },
  };
}

/** Approximate sRGB of black-body light (Tanner Helland's fit), for the lamp icon and slider. */
export function kelvinToRgb(kelvin: number): [number, number, number] {
  const t = kelvin / 100;
  const clamp = (v: number) => Math.round(Math.min(255, Math.max(0, v)));
  const r = t <= 66 ? 255 : 329.698727446 * Math.pow(t - 60, -0.1332047592);
  const g = t <= 66 ? 99.4708025861 * Math.log(t) - 161.1195681661 : 288.1221695283 * Math.pow(t - 60, -0.0755148492);
  const b = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  return [clamp(r), clamp(g), clamp(b)];
}

/** CSS colour the lamp is shining with right now. */
export function lampColor(s: LampState) {
  if (s.colorMode === 1) return `#${s.rgb.toString(16).padStart(6, "0")}`;
  const [r, g, b] = kelvinToRgb(s.ct || 4000);
  return `rgb(${r} ${g} ${b})`;
}
