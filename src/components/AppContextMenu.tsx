import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import {
  EyeOff,
  Eye,
  FileText,
  FolderMinus,
  FolderOpen,
  FolderX,
  History,
  Pencil,
  Pin,
  PinOff,
  Play,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useQuery } from "@tanstack/react-query";
import { FILE_PREFIX, fileEntry, isFileId, launchApp, useAppsById, useIsPinned } from "../lib/apps";
import { FOLDER_PREFIX, folderOf, usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";

interface AppInfo {
  exePath: string | null;
  recent: { name: string; path: string }[];
  trackingOff: boolean;
}

const item = "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-ink/10";
const icon = "size-4 shrink-0 text-fg-muted";

function Item({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button className={item} onClick={onClick}>
      {children}
    </button>
  );
}

/** Its name, edited in place of the menu. */
function Rename({ initial, onDone }: { initial: string; onDone: (name: string | null) => void }) {
  const [name, setName] = useState(initial);
  return (
    <form
      className="p-1"
      onSubmit={(e) => {
        e.preventDefault();
        onDone(name);
      }}
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onFocus={(e) => e.currentTarget.select()}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onDone(null);
          }
        }}
        className="h-8 w-full rounded-lg border border-stroke bg-field px-2 text-[13px] outline-none focus:border-accent/50"
      />
      <p className="px-1 pt-1.5 text-[11px] text-fg-subtle">Enter — сохранить, пусто — как было</p>
    </form>
  );
}

export function AppContextMenu() {
  const menu = usePanelStore((s) => s.menu);
  const setMenu = usePanelStore((s) => s.setMenu);
  const apps = useAppsById();
  const id = menu?.appId ?? "";
  const pinned = useIsPinned(id);
  const prefs = usePrefs();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [renaming, setRenaming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isFolder = id.startsWith(FOLDER_PREFIX);
  const isFile = isFileId(id);
  const info = useQuery({
    queryKey: ["app-info", id],
    queryFn: () => invoke<AppInfo>("app_info", { id }),
    enabled: !!menu && !isFolder,
    staleTime: 30_000,
  }).data;

  useEffect(() => {
    setRenaming(false);
    setError(null);
  }, [menu]);

  // Keep the menu inside the window; its content grows once the app's info arrives.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const { width, height } = ref.current.getBoundingClientRect();
    const x = Math.max(8, Math.min(menu.x, window.innerWidth - width - 8));
    const y = Math.max(8, Math.min(menu.y, window.innerHeight - height - 8));
    setPos((p) => (p.x === x && p.y === y ? p : { x, y }));
  });

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
  const done = () => setMenu(null);
  const folderKey = isFolder ? id.slice(FOLDER_PREFIX.length) : null;
  const folder = folderKey ? prefs.folders[folderKey] : null;
  const app = isFolder ? null : isFile ? fileEntry(id) : apps.get(id);
  const name = folder?.name ?? app?.name ?? "";
  const inFolder = !isFolder && folderOf(prefs.folders, id) != null;
  const hidden = prefs.hiddenApps.includes(id);
  const run = (p: Promise<unknown>) => p.then(done).catch((e) => setError(String(e)));

  let body: ReactNode;
  if (renaming) {
    body = (
      <Rename
        initial={name}
        onDone={(value) => {
          if (value != null) {
            if (folderKey) prefs.renameFolder(folderKey, value);
            else prefs.renameApp(id, value);
          }
          done();
        }}
      />
    );
  } else if (folderKey) {
    body = (
      <>
        <Item onClick={() => setRenaming(true)}>
          <Pencil className={icon} /> Переименовать
        </Item>
        <Item
          onClick={() => {
            prefs.dissolveFolder(folderKey);
            done();
          }}
        >
          <FolderX className={icon} /> Разобрать папку
        </Item>
      </>
    );
  } else {
    body = (
      <>
        <Item onClick={() => launchApp(id)}>
          <Play className={icon} /> Открыть
        </Item>
        {info?.exePath && !isFile && (
          <Item
            onClick={() => {
              run(invoke("launch_app_admin", { id }).then(() => {
                prefs.recordLaunch(id);
                usePanelStore.getState().setOpen(false);
              }));
            }}
          >
            <ShieldCheck className={icon} /> От имени администратора
          </Item>
        )}
        {(isFile || info?.exePath) && (
          <Item onClick={() => run(revealItemInDir(isFile ? id.slice(FILE_PREFIX.length) : info!.exePath!))}>
            <FolderOpen className={icon} /> {isFile ? "Показать в папке" : "Расположение файла"}
          </Item>
        )}

        {info && info.recent.length > 0 && (
          <div className="mt-1 border-t border-ink/10 pt-1">
            <div className="flex items-center gap-1.5 px-2.5 pt-0.5 pb-1 text-[11px] text-fg-subtle">
              <History className="size-3" /> Недавние
            </div>
            {info.recent.map((r) => (
              <button
                key={r.path}
                className={item}
                title={r.path}
                onClick={() => run(invoke("open_recent", { id, path: r.path }).then(() => usePanelStore.getState().setOpen(false)))}
              >
                <FileText className={icon} />
                <span className="min-w-0 flex-1 truncate">{r.name}</span>
              </button>
            ))}
          </div>
        )}
        {info && info.recent.length === 0 && info.trackingOff && !isFile && (
          <button
            className="mt-1 w-full border-t border-ink/10 px-2.5 pt-1.5 pb-1 text-left text-[11px] leading-snug text-fg-subtle hover:text-fg"
            onClick={() => run(openUrl("ms-settings:personalization-start"))}
          >
            Недавние файлы не видны: в Windows выключено «Показывать недавно открытые элементы». Открыть параметры →
          </button>
        )}

        <div className="mt-1 border-t border-ink/10 pt-1">
          <Item
            onClick={() => {
              prefs.togglePin(id);
              done();
            }}
          >
            {pinned ? <PinOff className={icon} /> : <Pin className={icon} />}
            {pinned ? "Открепить" : "Закрепить"}
          </Item>
          {inFolder && (
            <Item
              onClick={() => {
                prefs.ungroup(id);
                done();
              }}
            >
              <FolderMinus className={icon} /> Убрать из папки
            </Item>
          )}
          <Item onClick={() => setRenaming(true)}>
            <Pencil className={icon} /> Переименовать
          </Item>
          {!isFile && (
            <Item
              onClick={() => {
                prefs.setAppHidden(id, !hidden);
                done();
              }}
            >
              {hidden ? <Eye className={icon} /> : <EyeOff className={icon} />}
              {hidden ? "Показывать в списке" : "Скрыть из списка"}
            </Item>
          )}
          {!isFile && (
            <Item onClick={() => run(openUrl("ms-settings:appsfeatures"))}>
              <Trash2 className={icon} /> Удалить…
            </Item>
          )}
        </div>
      </>
    );
  }

  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.1 }}
      style={{ left: pos.x, top: pos.y }}
      className="fixed z-50 w-60 origin-top-left rounded-xl border border-ink/10 bg-popover p-1 shadow-2xl shadow-black/50"
    >
      {name && <div className="truncate px-2.5 pt-1 pb-1.5 text-[11px] text-fg-subtle">{name}</div>}
      {body}
      {error && <p className="px-2.5 pt-1 pb-1.5 text-[11.5px] leading-snug text-warn">{error}</p>}
    </motion.div>
  );
}
