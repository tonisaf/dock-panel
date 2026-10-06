import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, ExternalLink, History, Minus, Plus, RefreshCw, Sparkles, StickyNote, X } from "lucide-react";
import { useNotesActions } from "../notes/api";
import { usePanelStore } from "../store";

// Serialize hide/show across component mounts so a late hide cannot cover a restored chat.
let browserTasks = Promise.resolve();

export function GoogleBrowser({ query }: { query: string }) {
  const area = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");
  const [chats, setChats] = useState<{ id: number; title: string; url: string | null }[] | null>(null);
  const [reading, setReading] = useState(false);
  const [saving, setSaving] = useState(false);
  const { create, sync: syncNotes } = useNotesActions();
  const [zoom, setZoom] = useState(() => {
    const stored = Number(localStorage.getItem("google-ai-zoom"));
    return stored >= 0.5 && stored <= 2 ? stored : 1;
  });
  const zoomRef = useRef(zoom);
  const changeZoom = async (value: number) => {
    const next = Math.max(0.5, Math.min(2, Math.round(value * 100) / 100));
    try {
      await invoke("google_ai_zoom", { zoom: next });
      zoomRef.current = next; setZoom(next); localStorage.setItem("google-ai-zoom", String(next));
    } catch (e) { setError(String(e)); }
  };
  const saveSelection = async () => {
    if (saving) return;
    setSaving(true); setError("");
    try {
      const selected = await invoke<{text: string; url: string}>("google_ai_selection");
      const note = await create(selected.text.trim().split("\n")[0].slice(0, 100), `${selected.text.trim()}\n\nИсточник: ${selected.url}`);
      void syncNotes(false).catch(console.error);
      usePanelStore.getState().setTab("notes");
      usePanelStore.getState().openNote(note.id);
    } catch (e) { setError(String(e)); }
    finally { setSaving(false); }
  };
  const readChats = async () => {
    setReading(true);
    setError("");
    try { setChats(await invoke("google_ai_history")); }
    catch (e) { setError(String(e)); }
    finally { setReading(false); }
  };
  const request = usePanelStore((s) => s.googleRequest);
  useEffect(() => {
    const apply = () => invoke("google_ai_theme", { dark: document.documentElement.dataset.theme === "dark" }).catch(console.error);
    void apply();
    const observer = new MutationObserver(() => { void apply(); });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const element = area.current;
    if (!element) return;
    let disposed = false;
    const sync = (navigate = false) => {
      browserTasks = browserTasks.then(async () => {
        if (disposed) return;
        const rect = element.getBoundingClientRect();
        if (rect.width < 1 || rect.height < 1) return;
        const shouldNavigate = navigate && usePanelStore.getState().googleLoadedRequest !== request;
        await invoke("google_ai_embed", { query, x: rect.x, y: rect.y, width: rect.width, height: rect.height, navigate: shouldNavigate });
        await invoke("google_ai_zoom", { zoom: zoomRef.current });
        await invoke("google_ai_theme", { dark: document.documentElement.dataset.theme === "dark" });
        if (shouldNavigate) usePanelStore.setState({ googleLoadedRequest: request });
      }).catch((e) => { if (!disposed) setError(String(e)); });
    };
    setError("");
    sync(true);
    const settled = window.setTimeout(() => sync(), 250);
    const observer = new ResizeObserver(() => sync());
    observer.observe(element);
    window.addEventListener("resize", resize);
    function resize() { sync(); }
    return () => {
      disposed = true;
      window.clearTimeout(settled);
      observer.disconnect();
      window.removeEventListener("resize", resize);
    };
  }, [query, request]);
  const action = (action: string) => invoke("google_ai_action", { action }).catch((e) => setError(String(e)));
  const tool = "grid size-9 shrink-0 place-items-center rounded-lg text-fg-muted transition-colors hover:bg-ink/8 hover:text-fg focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50";
  return <div className="flex min-h-0 flex-1 flex-col gap-2 rounded-2xl border border-stroke bg-surface p-1.5">
    <div className="flex shrink-0 flex-wrap items-center gap-1 px-1 text-[13px]">
      <button title="Вернуться к приложениям" className="flex h-9 shrink-0 items-center gap-2 rounded-lg px-2 text-fg-muted transition-colors hover:bg-ink/8 focus-visible:outline-2 focus-visible:outline-accent" onClick={() => usePanelStore.getState().setTab("apps")}><ArrowLeft className="size-4" /><span className="hidden sm:inline">Приложения</span></button>
      <span className="mx-1 h-5 w-px shrink-0 bg-stroke" />
      <div className="flex min-w-0 items-center gap-2 px-1 font-medium"><Sparkles className="size-4 shrink-0 text-accent" /><span className="truncate">Google AI</span></div>
      <div className="ml-auto flex shrink-0 items-center gap-0.5">
        <button aria-label="Уменьшить масштаб" title="Уменьшить масштаб" disabled={zoom <= 0.5} className={tool} onClick={() => changeZoom(zoom - 0.1)}><Minus className="size-4" /></button>
        <button title="Сбросить масштаб до 100%" aria-label="Сбросить масштаб до 100%" className="h-9 min-w-12 rounded-lg px-1 text-[12px] tabular-nums hover:bg-ink/8" onClick={() => changeZoom(1)}>{Math.round(zoom * 100)}%</button>
        <button aria-label="Увеличить масштаб" title="Увеличить масштаб" disabled={zoom >= 2} className={tool} onClick={() => changeZoom(zoom + 0.1)}><Plus className="size-4" /></button>
        <button aria-label="Выделенный текст в заметку" title="Выделенный текст → заметка" disabled={saving} className={tool} onMouseDown={e => e.preventDefault()} onClick={saveSelection}><StickyNote className="size-4" /></button>
        <button aria-label="Назад на странице Google" title="Назад на странице Google" className={tool} onClick={() => action("back")}><ArrowLeft className="size-4" /></button>
        <button aria-label="Обновить страницу" title="Обновить страницу" className={tool} onClick={() => action("reload")}><RefreshCw className="size-4" /></button>
        <button aria-label="Чаты Google" title="Чаты Google" aria-expanded={chats !== null} disabled={reading} className={tool} onClick={() => chats ? setChats(null) : readChats()}><History className={reading ? "size-4 animate-spin" : "size-4"} /></button>
        <button aria-label="Открыть во внешнем браузере" title="Открыть во внешнем браузере" className={tool} onClick={() => action("external")}><ExternalLink className="size-4" /></button>
      </div>
    </div>
    {error && <div role="alert" className="text-[13px] text-red-500">{error}</div>}
    {chats && <div className="max-h-40 shrink-0 overflow-auto rounded-lg bg-surface p-2 text-[13px]">
      <div className="mb-1 flex items-center justify-between text-fg-subtle"><span>Чаты Google · {chats.length}</span><button aria-label="Закрыть список чатов" title="Закрыть список чатов" className={tool} onClick={() => setChats(null)}><X className="size-4" /></button></div>
      {!chats.length && <p>Раскройте боковую панель Google с разделом «Недавнее» и нажмите «Чаты Google» ещё раз.</p>}
      {chats.map(chat => <button key={chat.id} className="block w-full truncate rounded px-2 py-1.5 text-left hover:bg-ink/10" onClick={() => {
        void invoke("google_ai_chat", { id: chat.id }).catch(e => setError(String(e)));
      }}>{chat.title}</button>)}
    </div>}
    <div ref={area} className="min-h-0 flex-1 bg-browser" />
  </div>;
}
