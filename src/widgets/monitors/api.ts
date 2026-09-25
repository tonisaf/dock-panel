import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface Level {
  value: number;
  max: number;
}

export interface Choice {
  current: number;
  options: number[];
}

export interface Monitor {
  /** `\\.\DISPLAY1#0`: GDI device plus physical-monitor index. */
  id: string;
  name: string;
  primary: boolean;
  brightness: Level | null;
  contrast: Level | null;
  volume: Level | null;
  input: Choice | null;
  power: Choice | null;
}

export type Feature = "brightness" | "contrast" | "volume" | "input" | "power";

const KEY = ["monitors"];

export function useMonitors() {
  return useQuery({
    queryKey: KEY,
    queryFn: () => invoke<Monitor[]>("monitors_list"),
    // Values also change from the monitor's own buttons.
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
}

/** Sets a value right away in the cache; refetches if the monitor refuses. */
export function useSetMonitor() {
  const queryClient = useQueryClient();
  return async (monitor: Monitor, feature: Feature, value: number) => {
    queryClient.setQueryData<Monitor[]>(KEY, (list) =>
      list?.map((m) => {
        if (m.id !== monitor.id) return m;
        const current = m[feature];
        if (!current) return m;
        return { ...m, [feature]: "max" in current ? { ...current, value } : { ...current, current: value } };
      }),
    );
    try {
      await invoke("monitor_set", { id: monitor.id, feature, value });
    } catch (e) {
      await queryClient.invalidateQueries({ queryKey: KEY });
      throw e;
    }
  };
}

/** MCCS input source codes (VCP 0x60). */
const INPUTS: Record<number, string> = {
  0x01: "VGA",
  0x02: "VGA 2",
  0x03: "DVI",
  0x04: "DVI 2",
  0x0f: "DisplayPort",
  0x10: "DisplayPort 2",
  0x11: "HDMI",
  0x12: "HDMI 2",
  0x1b: "USB-C",
};

export const inputName = (code: number) => INPUTS[code] ?? `Вход ${code}`;

export const percent = (l: Level) => Math.round((l.value / l.max) * 100);
