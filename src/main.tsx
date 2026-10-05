import React from "react";
import ReactDOM from "react-dom/client";
import { focusManager, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MotionConfig } from "motion/react";
import { Panel } from "./shell/Panel";
import { DesktopWidget } from "./desktop/DesktopWidget";
import { GridOverlay } from "./desktop/GridOverlay";
import { deskWidget, isGridOverlay } from "./lib/desktop";
import "./index.css";
import { usePanelStore } from "./store";

if (deskWidget) document.documentElement.dataset.desk = "";
if (isGridOverlay) document.documentElement.dataset.grid = "";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false, staleTime: 60_000 } },
});

// Native hide() does not reliably change document.visibilityState in WebView2.
// Desktop widgets have their own clients and continue updating independently.
if (!deskWidget && !isGridOverlay) {
  focusManager.setFocused(usePanelStore.getState().open);
  usePanelStore.subscribe((state, previous) => {
    if (state.open !== previous.open) focusManager.setFocused(state.open);
  });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      {/* Windows' "Animation effects" off: movement goes, fades stay. */}
      <MotionConfig reducedMotion="user">
        {isGridOverlay ? <GridOverlay /> : deskWidget ? <DesktopWidget id={deskWidget} /> : <Panel />}
      </MotionConfig>
    </QueryClientProvider>
  </React.StrictMode>,
);
