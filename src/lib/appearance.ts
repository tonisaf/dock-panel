import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { usePrefs } from "./prefs";

function useSystemDark() {
  const query = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return dark;
}

/** Applies theme and accent to <html>; mount once near the root. */
export function useAppearance() {
  const { theme, systemAccent } = usePrefs();
  const systemDark = useSystemDark();
  const resolved = theme === "system" ? (systemDark ? "dark" : "light") : theme;
  const { data: accent } = useQuery({
    queryKey: ["system-accent"],
    queryFn: () => invoke<{ light: string; dark: string }>("system_accent"),
    enabled: systemAccent,
    // Accent changes are rare; re-read when the panel is reopened after a while.
    staleTime: 5 * 60_000,
  });

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = resolved;
    if (systemAccent && accent) {
      // Lighter shade reads on dark glass, darker shade on light glass.
      root.style.setProperty("--color-accent", resolved === "dark" ? accent.light : accent.dark);
    } else {
      root.style.removeProperty("--color-accent");
    }
  }, [resolved, systemAccent, accent]);
}
