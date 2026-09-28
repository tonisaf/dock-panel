import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useQuery, useQueryClient } from "@tanstack/react-query";

/** Must match `PREFIX` in desktop.rs. */
const PREFIX = "desk-";

/**
 * The widget this window shows on the desktop, or null in the panel itself:
 * each desktop widget is a window of its own, labelled after it.
 */
export const deskWidget = (() => {
  const label = getCurrentWebviewWindow().label;
  return label.startsWith(PREFIX) ? label.slice(PREFIX.length) : null;
})();

/** This window draws the desktop grid while a widget is dragged (`GRID_LABEL` in desktop.rs). */
export const isGridOverlay = getCurrentWebviewWindow().label === "deskgrid";

/** Ids of the widgets on the desktop, and a switch for one. */
export function useDesktopWidgets() {
  const queryClient = useQueryClient();
  const { data = [] } = useQuery({
    queryKey: ["desktop-widgets"],
    queryFn: () => invoke<string[]>("desktop_widgets"),
    staleTime: Infinity,
  });
  useEffect(() => {
    const unlisten = listen<string[]>("desktop:changed", ({ payload }) =>
      queryClient.setQueryData(["desktop-widgets"], payload),
    );
    return () => void unlisten.then((fn) => fn());
  }, [queryClient]);
  const set = (id: string, on: boolean) =>
    invoke<string[]>("desktop_set", { id, on })
      .then((ids) => queryClient.setQueryData(["desktop-widgets"], ids))
      .catch(console.error);
  return { onDesktop: data, set };
}
