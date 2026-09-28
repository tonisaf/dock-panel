import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MotionConfig } from "motion/react";
import { Panel } from "./shell/Panel";
import { DesktopWidget } from "./desktop/DesktopWidget";
import { GridOverlay } from "./desktop/GridOverlay";
import { deskWidget, isGridOverlay } from "./lib/desktop";
import "./index.css";

if (deskWidget) document.documentElement.dataset.desk = "";
if (isGridOverlay) document.documentElement.dataset.grid = "";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false, staleTime: 60_000 } },
});

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
