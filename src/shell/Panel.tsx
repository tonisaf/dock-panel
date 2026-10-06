import { useEffect, useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TABS, usePanelStore, type MailToOpen, type TabId } from "../store";
import { useApps } from "../lib/apps";
import { useAppearance } from "../lib/appearance";
import { usePanelSettings } from "../lib/panelWidth";
import { SearchBar } from "./SearchBar";
import { WindowButtons } from "./WindowButtons";
import { TabBar } from "./TabBar";
import { ResizeHandle } from "./ResizeHandle";
import { UpdateBanner } from "./UpdateBanner";
import { DropToPin } from "./DropToPin";
import { QuickAccess } from "./QuickAccess";
import { AppContextMenu } from "../components/AppContextMenu";
import { HomeTab } from "../tabs/HomeTab";
import { AppsTab } from "../tabs/AppsTab";
import { TasksTab } from "../tabs/TasksTab";
import { AiTab } from "../tabs/AiTab";
import { NotesTab } from "../tabs/NotesTab";
import { SettingsTab } from "../tabs/SettingsTab";
import { MailTab } from "../tabs/MailTab";
import { GoogleBrowser } from "./GoogleBrowser";
import { CalendarTab } from "../gcal/CalendarTab";
import { useDataEvents } from "../lib/dataEvents";

const TAB_VIEWS = {
  home: HomeTab,
  apps: AppsTab,
  tasks: TasksTab,
  mail: MailTab,
  calendar: CalendarTab,
  ai: AiTab,
  notes: NotesTab,
  settings: SettingsTab,
};

/**
 * Rust shows the window and emits `panel:show`; content then slides in.
 * On `panel:hide` (or Esc) content slides out, and only once the exit
 * animation completes do we ask Rust to actually hide the window.
 */
export function Panel() {
  const searchRef = useRef<HTMLInputElement>(null);
  const { open, setOpen, tab, setTab, setQuery, full, sideTab } = usePanelStore();
  const googleQuery = usePanelStore((s) => s.googleQuery);
  const googleActive = usePanelStore((s) => s.googleActive);
  useEffect(() => {
    void invoke("google_ai_visible", { visible: googleActive && open }).catch(console.error);
    return () => { void invoke("google_ai_visible", { visible: false }).catch(console.error); };
  }, [googleActive, open]);
  // Keep the app list warm so the Apps tab and search are instant.
  useApps();
  useAppearance();
  useDataEvents();
  // Slide in from the docked edge.
  const dir = usePanelSettings().edge === "right" ? 1 : -1;

  // The window's mode lives in Rust (it survives a page reload); start from it.
  useEffect(() => {
    invoke<boolean>("panel_fullscreen")
      .then((full) => usePanelStore.setState({ full }))
      .catch(console.error);
  }, []);

  useEffect(() => {
    const unlisten = [
      listen("panel:show", () => {
        setQuery("");
        setOpen(true);
        // Fresh unread counts right away instead of at the next minute tick.
        invoke("mail_refresh").catch(console.error);
      }),
      // Taskbar counter and notification clicks open a tab (and maybe a letter).
      listen<TabId>("panel:tab", ({ payload }) => setTab(payload)),
      // A note clicked in the notes widget on the desktop.
      listen<string>("panel:note", ({ payload }) => usePanelStore.getState().openNote(payload)),
      listen<MailToOpen>("mail:open", ({ payload }) => usePanelStore.getState().setMailToOpen(payload)),
      // Rust decided (hotkey, tray, click elsewhere when not pinned): hide even if pinned.
      listen("panel:hide", () => usePanelStore.getState().hide()),
    ];
    let disposed = false;
    if (import.meta.env.DEV) {
      Promise.all(unlisten).then(() => {
        if (!disposed) {
          invoke("panel_set_pinned", { on: true }).then(() =>
            invoke("panel_open", { tab: "apps", note: null }),
          ).catch(console.error);
        }
      });
    }
    return () => {
      disposed = true;
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
  }, [setOpen, setQuery, setTab]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const state = usePanelStore.getState();
      if (e.key === "Escape") {
        // Peel back one layer at a time: menu, then query, then the panel.
        if (state.googleActive) usePanelStore.getState().setTab("apps");
        else if (state.menu) state.setMenu(null);
        else if ((state.full ? state.sideTab : state.tab) === "mail" && state.mailQuery) state.setMailQuery("");
        else if ((state.full ? state.sideTab : state.tab) !== "mail" && state.query) state.setQuery("");
        else setOpen(false);
      } else if (e.ctrlKey && e.key >= "1" && e.key <= String(TABS.length)) {
        e.preventDefault();
        setTab(TABS[Number(e.key) - 1].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen, setTab]);

  const View = TAB_VIEWS[tab];
  const SideView = TAB_VIEWS[sideTab === "home" ? "apps" : sideTab];

  return (
    <AnimatePresence onExitComplete={() => invoke("hide_panel")}>
      {open && (
        <motion.div
          key="panel"
          className="relative flex h-full flex-col gap-3 p-4"
          initial={{ x: 32 * dir, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: 24 * dir, opacity: 0, transition: { duration: 0.14, ease: "easeIn" } }}
          transition={{ type: "spring", stiffness: 480, damping: 36, mass: 0.8 }}
          onAnimationStart={() => searchRef.current?.focus()}
        >
          <UpdateBanner />
          <div className="flex shrink-0 gap-2">
            <SearchBar ref={searchRef} />
            <WindowButtons />
          </div>
          {!googleActive && googleQuery && (full ? sideTab : tab) === "apps" && <button
            className="self-start rounded-lg bg-surface px-3 py-2 text-[13px]"
            onClick={() => usePanelStore.setState({ googleActive: true })}
          >Вернуться в Google AI</button>}
          {googleActive && googleQuery ? (<>
            <TabBar />
            <GoogleBrowser query={googleQuery} />
          </>) : full ? (
            // Full screen: home stays on the left, the other tabs switch on the right.
            <div className="flex min-h-0 flex-1 gap-4">
              <main className="scroll-area -mx-1 min-h-0 w-[52%] min-w-[360px] shrink-0 px-1">
                <HomeTab />
              </main>
              <div className="flex min-w-0 flex-1 flex-col gap-3">
                <TabBar exclude="home" active={sideTab} />
                <AnimatePresence mode="wait" initial={false}>
                  <motion.main
                    key={sideTab}
                    className="scroll-area -mx-1 min-h-0 flex-1 px-1"
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, transition: { duration: 0.05 } }}
                    transition={{ duration: 0.14, ease: "easeOut" }}
                  >
                    <SideView />
                  </motion.main>
                </AnimatePresence>
              </div>
            </div>
          ) : (
            <>
              <TabBar />
              <AnimatePresence mode="wait" initial={false}>
                <motion.main
                  key={tab}
                  className="scroll-area -mx-1 min-h-0 flex-1 px-1"
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, transition: { duration: 0.05 } }}
                  transition={{ duration: 0.14, ease: "easeOut" }}
                >
                  <View />
                </motion.main>
              </AnimatePresence>
            </>
          )}
          <QuickAccess />
          <AppContextMenu />
          <DropToPin />
          {!full && <ResizeHandle />}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
