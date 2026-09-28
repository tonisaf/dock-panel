import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { invalidateMail } from "../mail/api";
import { invalidateYoutube } from "../widgets/youtube/api";
import { AGENTS_KEY } from "../agents/api";

/** Refetches data Rust says has changed; mount once per window (the panel, each desktop widget). */
export function useDataEvents() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const unlisten = [
      listen("mail:changed", () => invalidateMail(queryClient)),
      listen("youtube:changed", () => invalidateYoutube(queryClient)),
      listen("ai-limits:changed", () => queryClient.invalidateQueries({ queryKey: ["ai-limits"] })),
      listen("agents:changed", () => queryClient.invalidateQueries({ queryKey: AGENTS_KEY })),
    ];
    return () => unlisten.forEach((p) => p.then((fn) => fn()));
  }, [queryClient]);
}
