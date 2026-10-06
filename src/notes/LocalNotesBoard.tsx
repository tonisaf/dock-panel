import { imageUrl, imageDropTarget, importImages, mergeImages, type NoteImage } from "./localNoteImages";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { textToChecklist, checklistToText } from "./localNoteModel";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, CheckSquare, Loader2, Pin, Plus, RotateCcw, Search, StickyNote, Trash2, ImagePlus, X } from "lucide-react";
import { usePanelStore } from "../store";

interface Item { id: string; text: string; checked: boolean }
interface LocalNote { id: string; title: string; body: string; kind: "text" | "list"; items: Item[]; images?: NoteImage[]; color: string; pinned: boolean; trashed: boolean; created: number; edited: number; revision: number }
const KEY = ["local-notes"];
const call = <T,>(op: string, args: Record<string, unknown> = {}) => invoke<T>("local_notes_command", { op, args });
const COLORS = ["default", "yellow", "green", "blue", "pink", "purple"] as const;
const LABELS = ["По теме панели", "Жёлтый", "Зелёный", "Голубой", "Розовый", "Фиолетовый"];
const backgrounds: Record<string, string> = { default: "var(--color-surface)", yellow: "color-mix(in srgb, #eab308 18%, var(--color-surface))", green: "color-mix(in srgb, #22c55e 16%, var(--color-surface))", blue: "color-mix(in srgb, #38bdf8 18%, var(--color-surface))", pink: "color-mix(in srgb, #fb7185 18%, var(--color-surface))", purple: "color-mix(in srgb, #a78bfa 20%, var(--color-surface))" };
const iconButton = "grid size-8 shrink-0 place-items-center rounded-lg text-fg-muted hover:bg-ink/10 disabled:opacity-50";
const draftKey = (id: string) => `local-note-draft:${id}`;
function storeDraft(note: LocalNote) { try { localStorage.setItem(draftKey(note.id), JSON.stringify(note)); } catch { /* Saving to SQLite still works. */ } }
function removeDraft(id: string) { try { localStorage.removeItem(draftKey(id)); } catch { /* Optional recovery cache. */ } }
function readDraft(note: LocalNote): LocalNote {
  try {
    const draft = JSON.parse(localStorage.getItem(draftKey(note.id)) ?? "null") as LocalNote | null;
    if (draft?.id === note.id && Array.isArray(draft.items) && typeof draft.title === "string" && typeof draft.body === "string") return draft;
  } catch { /* An invalid draft does not prevent opening a note. */ }
  return note;
}

function Editor({ note, onClose }: { note: LocalNote; onClose: () => void }) {
  const [value, setValue] = useState(() => readDraft(note));
  const latest = useRef(value);
  const dirty = useRef(value !== note);
  const running = useRef<Promise<boolean> | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [status, setStatus] = useState(dirty.current ? "Восстановлен черновик" : "Сохранено");
  const [error, setError] = useState("");
  const [closing, setClosing] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);
  const importing = useRef<Promise<void>>(Promise.resolve());
  const [imageBusy, setImageBusy] = useState(false);
  const [preview, setPreview] = useState<NoteImage | null>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (preview) previewRef.current?.focus(); }, [preview]);
  const fileInput = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const flush = () => {
    clearTimeout(timer.current);
    if (running.current) return running.current;
    const work = async () => {
      while (dirty.current) {
        dirty.current = false; setStatus("Сохраняю…");
        try {
          const saved = await call<LocalNote>("save", { note: latest.current });
          latest.current = { ...latest.current, revision: saved.revision, created: saved.created, edited: saved.edited };
          setValue(latest.current);
          if (dirty.current) storeDraft(latest.current); else removeDraft(saved.id);
          setError("");
          void queryClient.invalidateQueries({ queryKey: KEY });
        } catch (e) {
          dirty.current = true; storeDraft(latest.current); setError(String(e)); setStatus("Не сохранено"); return false;
        }
      }
      setStatus("Сохранено"); return true;
    };
    running.current = work().finally(() => { running.current = null; });
    return running.current;
  };
  const change = (patch: Partial<LocalNote>) => {
    const next = { ...latest.current, ...patch };
    latest.current = next; setValue(next); dirty.current = true; storeDraft(next); setStatus("Изменено");
    clearTimeout(timer.current); timer.current = setTimeout(() => void flush(), 450);
  };
  const close = async () => {
    setClosing(true);
    await importing.current;
    if (await flush()) onClose();
    setClosing(false);
  };
  useEffect(() => {
    const active = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector<HTMLInputElement>("input")?.focus();
    if (dirty.current) timer.current = setTimeout(() => void flush(), 450);
    return () => { clearTimeout(timer.current); void flush(); active?.focus(); };
    // The editor owns a single note until closed. Mutable refs always contain its latest draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const addImages = (sources: (File | string)[]) => {
    importing.current = importing.current.then(async () => {
      setImageBusy(true);
      try { const added = await importImages(sources); change({ images: mergeImages(latest.current.images ?? [], added) }); }
      catch (e) { setError(String(e)); }
      finally { setImageBusy(false); }
    });
  };
  useEffect(() => {
    const drop = (event: Event) => { const e = event as CustomEvent<{ id: string; paths: string[] }>; if (e.detail.id === note.id) addImages(e.detail.paths); };
    window.addEventListener("local-note-image-drop", drop);
    return () => window.removeEventListener("local-note-image-drop", drop);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [note.id]);
  const convert = () => {
    if (value.kind === "text") change({ kind: "list", body: "", items: textToChecklist(value.body, () => crypto.randomUUID()) });
    else change({ kind: "text", items: [], body: checklistToText(value.items) });
  };
  const updateItem = (id: string, patch: Partial<Item>) => change({ items: latest.current.items.map((i) => i.id === id ? { ...i, ...patch } : i) });
  return <div className="fixed inset-0 z-60 flex items-center justify-center bg-black/30 p-4" onMouseDown={(e) => { if (e.target === e.currentTarget) void close(); }}>
    <div ref={dialog} data-note-image-target={note.id} onPaste={(e) => { const files = Array.from(e.clipboardData.files); if (files.length) { e.preventDefault(); addImages(files); } }} onDragOver={(e) => { e.preventDefault(); }} onDrop={(e) => { e.preventDefault(); e.stopPropagation(); if (e.dataTransfer.files.length) addImages(Array.from(e.dataTransfer.files)); }} role="dialog" aria-modal="true" aria-label="Редактор заметки" style={{ background: backgrounds[value.color] }} className="flex max-h-[85vh] w-full max-w-xl flex-col rounded-2xl border border-stroke p-4 shadow-xl" onKeyDown={(e) => {
      if (e.key === "Escape") { e.stopPropagation(); if (preview) setPreview(null); else void close(); }
      if (e.ctrlKey && (e.key === "s" || e.key === "Enter")) { e.preventDefault(); void flush(); }
      if (e.key === "Tab") {
        const elements = Array.from(dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled),input:not(:disabled),textarea") ?? []);
        if (e.shiftKey && document.activeElement === elements[0]) { e.preventDefault(); elements[elements.length - 1]?.focus(); }
        else if (!e.shiftKey && document.activeElement === elements[elements.length - 1]) { e.preventDefault(); elements[0]?.focus(); }
      }
    }}>
      <div className="mb-3 flex items-center gap-2">
        <input aria-label="Заголовок" maxLength={250} placeholder="Заголовок" className="min-w-0 flex-1 bg-transparent text-lg font-semibold outline-none" value={value.title} onChange={(e) => change({ title: e.target.value })} />
        <button className={iconButton} title={value.pinned ? "Открепить" : "Закрепить"} aria-pressed={value.pinned} onClick={() => change({ pinned: !value.pinned })}><Pin className="size-4" fill={value.pinned ? "currentColor" : "none"} /></button>
        <button disabled={closing} className={iconButton} title="Закрыть" onClick={() => void close()}><X className="size-4" /></button>
      </div>
      <div className="min-h-40 overflow-y-auto">
        {(value.images ?? []).length > 0 && <div className="mb-3 grid grid-cols-2 gap-2">{value.images!.map((image) => <div key={image.id} className="relative"><button className="w-full" title="Открыть изображение" onClick={() => setPreview(image)}><img src={imageUrl(image)} alt={image.name} className="max-h-48 w-full rounded-lg object-cover" /></button><button title="Убрать изображение из заметки" className={iconButton + " absolute right-1 top-1 bg-popover"} onClick={() => change({ images: value.images!.filter((i) => i.id !== image.id) })}><X className="size-4" /></button></div>)}</div>}
        {imageBusy && <p className="mb-2 text-xs text-fg-subtle">Сохраняю изображения…</p>}

        {value.kind === "text" ? <textarea aria-label="Текст заметки" placeholder="Заметка…" className="min-h-56 w-full resize-y bg-transparent text-[14px] leading-relaxed outline-none" value={value.body} onChange={(e) => change({ body: e.target.value })} /> : <>
          {[false, true].map((checked) => <div key={String(checked)} className={checked && value.items.some((i) => i.checked) ? "mt-3 border-t border-stroke pt-3" : ""}>
            {checked && value.items.some((i) => i.checked) && <p className="mb-2 text-xs text-fg-subtle">Выполненные</p>}
            {value.items.filter((i) => i.checked === checked).map((item) => <div key={item.id} className="flex items-center gap-2 py-1">
              <input aria-label={`Выполнено: ${item.text}`} type="checkbox" checked={item.checked} onChange={(e) => updateItem(item.id, { checked: e.target.checked })} className="size-4 accent-accent" />
              <input aria-label="Пункт списка" maxLength={3000} value={item.text} onChange={(e) => updateItem(item.id, { text: e.target.value })} className={`min-w-0 flex-1 bg-transparent text-[14px] outline-none ${item.checked ? "text-fg-subtle line-through" : ""}`} />
              <button title="Удалить пункт" className={iconButton} onClick={() => change({ items: value.items.filter((i) => i.id !== item.id) })}><X className="size-3.5" /></button>
            </div>)}
          </div>)}
          <button className="mt-2 flex items-center gap-2 rounded-lg px-2 py-2 text-sm text-fg-muted hover:bg-ink/10" onClick={() => change({ items: [...value.items, { id: crypto.randomUUID(), text: "", checked: false }] })}><Plus className="size-4" />Пункт списка</button>
        </>}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-stroke pt-3">
        <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple className="hidden" onChange={(e) => { addImages(Array.from(e.target.files ?? [])); e.target.value = ""; }} />
        <button title="Добавить изображения" className={iconButton} disabled={imageBusy} onClick={() => fileInput.current?.click()}><ImagePlus className="size-4" /></button>
        {COLORS.map((color, i) => <button key={color} aria-label={LABELS[i]} aria-pressed={value.color === color} title={LABELS[i]} onClick={() => change({ color })} style={{ background: backgrounds[color] }} className="grid size-7 place-items-center rounded-full border border-ink/20">{value.color === color && <Check className="size-3.5" />}</button>)}
        <button onClick={convert} className="ml-auto rounded-lg px-2 py-1.5 text-xs hover:bg-ink/10">{value.kind === "text" ? "Сделать списком" : "Сделать текстом"}</button>
      </div>
      <p className="mt-2 text-[11px] text-fg-subtle">Изображение: Ctrl+V или перетащите сюда · до 20 МБ</p>
      <div className="mt-2 flex items-center gap-2 text-xs text-fg-subtle"><span aria-live="polite">{status}</span><button className="ml-auto rounded-lg px-2 py-1 hover:bg-ink/10" onClick={() => { change({ trashed: !value.trashed }); void close(); }}>{value.trashed ? "Восстановить" : "В корзину"}</button></div>
      {error && <div role="alert" className="mt-2 text-xs text-warn">{error}<div className="mt-1 flex gap-3"><button onClick={() => void flush()} className="underline">Повторить</button><button onClick={() => { storeDraft(latest.current); onClose(); }} className="underline">Закрыть, оставить черновик</button><button onClick={() => { void call<LocalNote>("get", { id: note.id }).then((saved) => { dirty.current = false; latest.current = saved; setValue(saved); removeDraft(saved.id); setError(""); setStatus("Сохранено"); }).catch((e) => setError(String(e))); }} className="underline">Отменить правки и перечитать</button></div></div>}
    </div>
    {preview && <div ref={previewRef} role="dialog" aria-modal="true" aria-label="Изображение" data-note-image-target={note.id} tabIndex={-1} className="fixed inset-0 z-70 flex items-center justify-center bg-black/85 p-8" onClick={() => setPreview(null)} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setPreview(null); } if (e.key === "Tab") { e.preventDefault(); previewRef.current?.querySelector<HTMLButtonElement>("button")?.focus(); } }}><img src={imageUrl(preview)} alt={preview.name} className="max-h-full max-w-full object-contain" /><button className="absolute right-4 top-4 rounded-lg bg-white/20 p-2 text-white" title="Закрыть изображение" onClick={() => setPreview(null)}><X /></button></div>}
  </div>;
}

export function LocalNotesBoard() {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: KEY, queryFn: () => call<LocalNote[]>("list"), staleTime: 30_000 });
  const [opened, setOpened] = useState<LocalNote | null>(null);
  const [dragTarget, setDragTarget] = useState<string | undefined>(undefined);
  const [trash, setTrash] = useState(false);
  const [search, setSearch] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const openedRef = useRef(opened); openedRef.current = opened;
  const noteToOpen = usePanelStore((s) => s.noteToOpen);
  const create = async (kind: "text" | "list") => {
    setBusy(true); setError("");
    try { const n = await call<LocalNote>("create", { title: "", body: "", kind }); setOpened(n); void query.refetch(); }
    catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  useEffect(() => {
    const off = listen("local-notes:changed", () => { void queryClient.invalidateQueries({ queryKey: KEY }); });
    return () => { void off.then((f) => f()); };
  }, [queryClient]);
  useEffect(() => {
    if (!noteToOpen) return;
    usePanelStore.getState().openNote(null);
    if (noteToOpen === "new") void create("text");
    else void call<LocalNote>("get", { id: noteToOpen }).then((n) => { if (!n.trashed) setOpened(n); }).catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [noteToOpen]);
  const change = async (note: LocalNote, patch: Partial<LocalNote>) => {
    setError("");
    try { await call("save", { note: { ...note, ...patch } }); await query.refetch(); }
    catch (e) { setError(String(e)); }
  };
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") { setDragTarget(undefined); return; }
      const id = imageDropTarget(payload.position.x, payload.position.y);
      if (payload.type !== "drop") { setDragTarget(id); return; }
      setDragTarget(undefined);
      if (!id) return;
      if (openedRef.current?.id === id) { window.dispatchEvent(new CustomEvent("local-note-image-drop", { detail: { id, paths: payload.paths } })); return; }
      void (async () => { try {
        const added = await importImages(payload.paths);
        const current = await call<LocalNote>("get", { id });
        await call("save", { note: { ...current, images: mergeImages(current.images ?? [], added) } });
        await queryClient.invalidateQueries({ queryKey: KEY });
      } catch (e) { setError(String(e)); } })();
    });
    return () => { void off.then((f) => f()); };
  }, [queryClient]);
  const q = search.trim().toLowerCase();
  const shown = (query.data ?? []).filter((n) => n.trashed === trash && `${n.title}\n${n.body}\n${n.items.map((i) => i.text).join("\n")}`.toLowerCase().includes(q));
  const cards = (notes: LocalNote[]) => <div className="[column-width:240px] [column-gap:14px]">{notes.map((n) => <article key={n.id} data-note-image-target={n.id} style={{ background: backgrounds[n.color] }} className={`group mb-3.5 break-inside-avoid rounded-xl border border-stroke p-3.5 ${dragTarget === n.id ? "ring-2 ring-accent" : ""}`}>
    <button className="w-full text-left" onClick={() => setOpened(n)}>
      {!!n.images?.length && <div className="mb-3 flex gap-1 overflow-hidden rounded-lg">{n.images.slice(0, 3).map((image) => <img key={image.id} src={imageUrl(image)} alt={image.name} className="max-h-48 min-w-0 flex-1 object-cover" />)}</div>}
      {n.title && <h3 className="mb-2 break-words text-[15px] font-semibold">{n.title}</h3>}
      {n.kind === "text" ? <p className="max-h-64 overflow-hidden whitespace-pre-wrap break-words text-[13px] leading-relaxed">{n.body || (!n.title ? "Пустая заметка" : "")}</p> : <div className="flex flex-col gap-1.5">{n.items.slice(0, 12).map((i) => <div key={i.id} className="flex items-start gap-2 text-[13px]"><span className="mt-0.5 grid size-3.5 shrink-0 place-items-center rounded-sm border border-ink/40">{i.checked && <Check className="size-3" />}</span><span className={i.checked ? "text-fg-subtle line-through" : ""}>{i.text || "Пункт списка"}</span></div>)}{n.items.length > 12 && <span className="text-xs text-fg-subtle">Ещё {n.items.length - 12}</span>}</div>}
    </button>
    <div className="mt-2 flex justify-end gap-1">
      {trash ? <button title="Восстановить" className={iconButton} onClick={() => void change(n, { trashed: false })}><RotateCcw className="size-4" /></button> : <>
        <button title={n.pinned ? "Открепить" : "Закрепить"} aria-pressed={n.pinned} className={iconButton} onClick={() => void change(n, { pinned: !n.pinned })}><Pin className="size-4" fill={n.pinned ? "currentColor" : "none"} /></button>
        <button title="В корзину" className={iconButton} onClick={() => void change(n, { trashed: true })}><Trash2 className="size-4" /></button>
      </>}
    </div>
  </article>)}</div>;
  return <div className="flex flex-col gap-4 p-2">
    <div className="flex flex-wrap items-center gap-2">
      <button disabled={busy || trash} onClick={() => void create("text")} className="flex min-h-11 min-w-32 flex-1 items-center gap-2 rounded-xl border border-stroke bg-field px-3 text-sm text-fg-muted hover:bg-surface"><Plus className="size-4" />Новая заметка…</button>
      <button disabled={busy || trash} title="Новый список" className={iconButton + " size-11 border border-stroke"} onClick={() => void create("list")}><CheckSquare className="size-4" /></button>
      <button title={trash ? "К заметкам" : "Корзина"} aria-pressed={trash} className={iconButton + " size-11 border border-stroke"} onClick={() => setTrash(!trash)}>{trash ? <StickyNote className="size-4" /> : <Trash2 className="size-4" />}</button>
    </div>
    <label className="flex items-center gap-2 rounded-lg border border-stroke bg-field px-3"><Search className="size-4 text-fg-subtle" /><input aria-label="Поиск заметок" value={search} onChange={(e) => setSearch(e.target.value)} placeholder={trash ? "Поиск в корзине" : "Поиск заметок"} className="h-9 min-w-0 flex-1 bg-transparent text-sm outline-none" /></label>
    {trash && <p className="text-xs text-fg-subtle">Корзина — заметки сохраняются, пока вы их не восстановите.</p>}
    {(error || query.error) && <p role="alert" className="text-sm text-warn">{error || String(query.error)} <button onClick={() => void query.refetch()} className="underline">Повторить</button></p>}
    {query.isPending ? <Loader2 className="mx-auto size-5 animate-spin" /> : !shown.length ? <p className="py-12 text-center text-sm text-fg-subtle">{q ? "Ничего не найдено" : trash ? "Корзина пуста" : "Создайте первую заметку или список"}</p> : <>
      {!trash && shown.some((n) => n.pinned) && <section><h2 className="mb-3 text-xs font-medium text-fg-subtle">Закреплённые</h2>{cards(shown.filter((n) => n.pinned))}</section>}
      <section>{!trash && shown.some((n) => n.pinned) && <h2 className="mb-3 text-xs font-medium text-fg-subtle">Остальные</h2>}{cards(shown.filter((n) => trash || !n.pinned))}</section>
    </>}
    {opened && <Editor key={opened.id} note={opened} onClose={() => setOpened(null)} />}
  </div>;
}
