import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { FolderOpen, Pin, PinOff, Play } from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FILE_PREFIX, fileEntry, isFileId, launchApp, useAppsById } from "../lib/apps";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";

export function AppContextMenu() {
  const menu = usePanelStore((s) => s.menu);
  const setMenu = usePanelStore((s) => s.setMenu);
  const apps = useAppsById();
  const pinned = usePrefs((s) => (menu ? s.pinned.includes(menu.appId) : false));
  const togglePin = usePrefs((s) => s.togglePin);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const { width, height } = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - width - 8),
      y: Math.min(menu.y, window.innerHeight - height - 8),
    });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) setMenu(null);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("wheel", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("wheel", close);
    };
  }, [menu, setMenu]);

  if (!menu) return null;
  const isFile = isFileId(menu.appId);
  const app = isFile ? fileEntry(menu.appId) : apps.get(menu.appId);

  const item = "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-ink/10";
  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.1 }}
      style={{ left: pos.x, top: pos.y }}
      className="fixed z-50 min-w-48 origin-top-left rounded-xl border border-ink/10 bg-popover p-1 shadow-2xl shadow-black/50"
    >
      {app && <div className="truncate px-2.5 pt-1 pb-1.5 text-[11px] text-fg-subtle">{app.name}</div>}
      <button className={item} onClick={() => launchApp(menu.appId)}>
        <Play className="size-4 text-fg-muted" /> Открыть
      </button>
      {isFile && (
        <button
          className={item}
          onClick={() => {
            revealItemInDir(menu.appId.slice(FILE_PREFIX.length)).catch(console.error);
            setMenu(null);
          }}
        >
          <FolderOpen className="size-4 text-fg-muted" /> Показать в папке
        </button>
      )}
      <button
        className={item}
        onClick={() => {
          togglePin(menu.appId);
          setMenu(null);
        }}
      >
        {pinned ? <PinOff className="size-4 text-fg-muted" /> : <Pin className="size-4 text-fg-muted" />}
        {pinned ? "Открепить" : "Закрепить"}
      </button>
    </motion.div>
  );
}
