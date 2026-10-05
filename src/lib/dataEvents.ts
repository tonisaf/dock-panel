import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { invalidateMail } from "../mail/api";
import { invalidateYoutube } from "../widgets/youtube/api";
import { AGENTS_KEY } from "../agents/api";
import { deskWidget } from "./desktop";
import { usePanelStore } from "../store";

/** Refetches data Rust says has changed; mount once per window (the panel, each desktop widget). */
export function useDataEvents() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const unlisten = [
      listen("mail:changed", () => invalidateMail(queryClient, refetchType())),
      listen("youtube:changed", () => invalidateYoutube(queryClient, refetchType())),
      listen("ai-limits:changed", () => queryClient.invalidateQueries({ queryKey: ["ai-limits"], refetchType: refetchType() })),
      listen("agents:changed", () => queryClient.invalidateQueries({ queryKey: AGENTS_KEY, refetchType: refetchType() })),
    ];
    return () => unlisten.forEach((p) => p.then((fn) => fn()));
  }, [queryClient]);
}

function refetchType(): "active" | "none" {
  return deskWidget || usePanelStore.getState().open ? "active" : "none";
}
