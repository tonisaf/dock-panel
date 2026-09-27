import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, CloudOff, Loader2, Pencil, Pin, Plus, RefreshCw, StickyNote, X } from "lucide-react";
import clsx from "clsx";
import { EmptyState } from "../components/Card";
import { SplitView, useSidePane } from "../components/SidePane";
import { usePanelSettings } from "../lib/panelWidth";
import { usePrefs } from "../lib/prefs";
import { usePanelStore } from "../store";
import { Blocks, OpenInNotion } from "../notes/NoteView";
import { NoteEditor, PinButton, TagEditor } from "../notes/NoteEditor";
import { ago, tagStyle, useNotePage, useNotes, useNotesActions, type Note, type NotesState } from "../notes/api";

function Status({ s, onRefresh }: { s: NotesState; onRefresh: () => void }) {
  const text = s.syncing
    ? "Синхронизирую…"
    : s.error
      ? `Нет связи с Notion — показан кэш${s.syncedAt ? ` от ${ago(s.syncedAt)}` : ""}`
      : s.syncedAt
        ? `Обновлено ${ago(s.syncedAt)}`
        : "Ещё не синхронизировано";
  return (
    <div className="flex items-center gap-1.5 text-[11.5px] text-fg-subtle">
      {s.error && !s.syncing && <CloudOff className="size-3.5 shrink-0 text-warn" />}
      <span className={clsx("min-w-0 truncate", s.error && !s.syncing && "text-warn")} title={s.error ?? undefined}>
        {text}
        {s.pending > 0 && ` · ждут отправки: ${s.pending}`}
      </span>
      <button onClick={onRefresh} title="Обновить из Notion" className="ml-auto grid size-6 shrink-0 place-items-center rounded-md hover:bg-ink/10 hover:text-fg">
        <RefreshCw className={clsx("size-3.5", s.syncing && "animate-spin")} />
      </button>
    </div>
  );
}

function Composer({ onDone }: { onDone: (note: Note | null) => void }) {
  const { create } = useNotesActions();
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setError(null);
    try {
      onDone(await create(title, body));
    } catch (e) {
      setError(String(e));
    }
  };
  const field = "w-full rounded-lg border border-stroke bg-field px-2.5 py-1.5 text-[13px] text-fg outline-none placeholder:text-fg-subtle focus:border-accent/50";
  return (
    <div
      className="flex flex-col gap-2 rounded-xl border border-stroke bg-surface p-2.5"
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.ctrlKey) save();
        if (e.key === "Escape") {
          e.stopPropagation();
          onDone(null);
        }
      }}
    >
      <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Заголовок" className={clsx(field, "font-medium")} />
      <textarea
        value={body}
        onChange={(e) => setBody(e.target.value)}
        rows={5}
        placeholder={"Текст. «- » — список, «[ ] » — чекбокс, «# » — заголовок"}
        className={clsx(field, "resize-y leading-relaxed")}
      />
      {error && <p className="text-[12px] text-warn">{error}</p>}
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-fg-subtle">Ctrl+Enter — сохранить. Без интернета уйдёт в Notion позже</span>
        <button onClick={() => onDone(null)} className="ml-auto rounded-lg px-2.5 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg">
          Отмена
        </button>
        <button
          onClick={save}
          disabled={!title.trim() && !body.trim()}
          className="rounded-lg bg-accent px-3 py-1 text-[12px] font-medium text-on-accent disabled:opacity-40"
        >
          Сохранить
        </button>
      </div>
    </div>
  );
}

function Row({ n, active, onOpen }: { n: Note; active: boolean; onOpen: () => void }) {
  return (
    <button
      onClick={onOpen}
      className={clsx("flex w-full flex-col gap-0.5 rounded-xl px-2.5 py-2 text-left transition-colors", active ? "bg-ink/10" : "hover:bg-surface")}
    >
      <div className="flex items-center gap-1.5 text-[13.5px]">
        {n.icon && <span className="shrink-0">{n.icon}</span>}
        <span className="min-w-0 flex-1 truncate font-medium">{n.title || "Без названия"}</span>
        {n.local && <span title="Ещё не в Notion"><CloudOff className="size-3.5 shrink-0 text-fg-subtle" /></span>}
        {n.pinned && <Pin className="size-3.5 shrink-0 text-fg-subtle" />}
        <span className="shrink-0 text-[11px] text-fg-subtle tabular-nums">{ago(n.edited)}</span>
      </div>
      {n.preview && <p className="line-clamp-2 text-[12px] leading-snug text-fg-subtle">{n.preview}</p>}
      {n.tags.length > 0 && (
        <div className="mt-0.5 flex flex-wrap gap-1">
          {n.tags.map((t) => (
            <span key={t.name} style={tagStyle(t.color)} className="rounded px-1.5 text-[10.5px] leading-4 text-fg-muted">
              {t.name}
            </span>
          ))}
        </div>
      )}
    </button>
  );
}

function Reader({ note, state, split, onBack }: { note: Note; state: NotesState; split: boolean; onBack: () => void }) {
  const { data: blocks, isPending, error } = useNotePage(note);
  const { toggle } = useNotesActions();
  const [toggleError, setToggleError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);

  if (editing) {
    return (
      <article className="flex flex-col gap-2 pt-0.5">
        <div className="flex items-center gap-1.5 text-[12px] text-fg-subtle">
          <Pencil className="size-3.5" /> Редактирование{note.local ? "" : " · в Notion уйдут только изменения"}
        </div>
        <NoteEditor note={note} onDone={() => setEditing(false)} />
      </article>
    );
  }
  return (
    <article className="flex flex-col gap-3 pb-4">
      <header className="flex items-start gap-1.5">
        <button onClick={onBack} title={split ? "Закрыть" : "Назад"} className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg">
          {split ? <X className="size-4" /> : <ArrowLeft className="size-4" />}
        </button>
        <div className="min-w-0 flex-1 pt-0.5">
          <h2 className="text-[17px] leading-snug font-semibold">
            {note.icon && <span className="mr-1.5">{note.icon}</span>}
            {note.title || "Без названия"}
          </h2>
          <div className="mt-0.5 text-[11.5px] text-fg-subtle">
            {note.local ? "Ещё не отправлена в Notion" : `Изменена ${ago(note.edited)}`}
          </div>
          {state.canTag && !note.local && (
            <div className="mt-1.5">
              <TagEditor note={note} options={state.tagOptions} />
            </div>
          )}
        </div>
        <button
          onClick={() => setEditing(true)}
          disabled={isPending || !!error}
          title="Редактировать"
          className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg disabled:opacity-40"
        >
          <Pencil className="size-3.5" />
        </button>
        {state.canPin && !note.local && <PinButton note={note} />}
        <OpenInNotion url={note.url} />
      </header>
      {toggleError && <p className="text-[12px] text-warn">{toggleError}</p>}
      {isPending ? (
        <p className="flex items-center gap-2 text-[12.5px] text-fg-subtle">
          <Loader2 className="size-4 animate-spin" /> Загружаю…
        </p>
      ) : error ? (
        <p className="text-[12.5px] text-warn">{String(error)}</p>
      ) : blocks?.length ? (
        <div className="text-[13.5px] leading-relaxed select-text">
          <Blocks
            blocks={blocks}
            onToggle={(id, checked) => {
              setToggleError(null);
              toggle(note, id, checked).catch((e) => setToggleError(String(e)));
            }}
          />
        </div>
      ) : (
        <p className="text-[12.5px] text-fg-subtle">Пустая заметка</p>
      )}
    </article>
  );
}

export function NotesTab() {
  const setTab = usePanelStore((s) => s.setTab);
  const noteToOpen = usePanelStore((s) => s.noteToOpen);
  const panelWidth = usePanelSettings().width;
  const baseWidth = usePanelStore((s) => s.full) ? Number.POSITIVE_INFINITY : panelWidth;
  const savedListWidth = usePrefs((s) => s.notesListWidth);
  const setNotesListWidth = usePrefs((s) => s.setNotesListWidth);
  const { data: s } = useNotes();
  const { sync } = useNotesActions();
  const pane = useSidePane<string>(baseWidth);
  const [composing, setComposing] = useState(false);
  const [tag, setTag] = useState<string | null>(null);

  // Opening the tab is a good moment to look for changes (skipped if the last sync is fresh).
  useEffect(() => {
    sync(false).catch(console.error);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per visit
  }, []);

  // A note asked for from search or the widget.
  useEffect(() => {
    if (!noteToOpen) return;
    if (noteToOpen === "new") setComposing(true);
    else void pane.open(noteToOpen);
    usePanelStore.getState().openNote(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when asked
  }, [noteToOpen]);

  const tags = useMemo(() => [...new Set((s?.notes ?? []).flatMap((n) => n.tags.map((t) => t.name)))].sort(), [s?.notes]);
  const shown = (s?.notes ?? []).filter((n) => !tag || n.tags.some((t) => t.name === tag));
  const reading = s?.notes.find((n) => n.id === pane.item) ?? null;

  // The open note went away (deleted in Notion, or a quick note replaced by the one Notion made).
  const gone = !!s && !!pane.item && !reading;
  useEffect(() => {
    if (gone) void pane.close();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when it goes
  }, [gone]);

  if (s && !s.configured) {
    return (
      <div className="flex h-full flex-col">
        <EmptyState
          icon={StickyNote}
          title="Заметки из Notion"
          text="Подключите Notion и выберите базу заметок в настройках. Заметки сохранятся на компьютере и будут открываться без интернета."
        />
        <button onClick={() => setTab("settings")} className="mx-auto -mt-12 text-[12.5px] text-accent hover:underline">
          Открыть настройки
        </button>
      </div>
    );
  }
  if (!s) return null;

  const close = () => void pane.close();
  const reader = reading && <Reader key={reading.id} note={reading} state={s} split={pane.mode === "split"} onBack={close} />;
  if (reader && pane.mode === "full") return reader;

  const list = (
    <div className="flex flex-col gap-2 pb-2">
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1">
          <Status s={s} onRefresh={() => sync(true).catch(console.error)} />
        </div>
        {!composing && (
          <button
            onClick={() => setComposing(true)}
            className="flex shrink-0 items-center gap-1 rounded-lg border border-stroke px-2 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg"
          >
            <Plus className="size-3.5" /> Заметка
          </button>
        )}
      </div>
      {composing && (
        <Composer
          onDone={(note) => {
            setComposing(false);
            if (note) void pane.open(note.id);
          }}
        />
      )}
      {tags.length > 0 && (
        <div className="flex gap-1.5 overflow-x-auto">
          {[null, ...tags].map((t) => (
            <button
              key={t ?? ""}
              onClick={() => setTag(t)}
              className={clsx(
                "shrink-0 rounded-full border px-2.5 py-0.5 text-[12px] transition-colors",
                tag === t ? "border-accent/50 bg-accent/15 text-fg" : "border-stroke text-fg-muted hover:text-fg",
              )}
            >
              {t ?? "Все"}
            </button>
          ))}
        </div>
      )}
      {shown.length === 0 ? (
        s.syncing ? (
          <p className="flex items-center gap-2 px-2.5 py-3 text-[12.5px] text-fg-subtle">
            <Loader2 className="size-4 animate-spin" /> Загружаю заметки из Notion…
          </p>
        ) : (
          <EmptyState icon={StickyNote} title="Заметок нет" text={tag ? "С этим тегом заметок нет." : "В базе пока пусто — создайте первую."} />
        )
      ) : (
        <div className="flex flex-col">
          {shown.map((n) => (
            <Row key={n.id} n={n} active={n.id === pane.item} onOpen={() => void pane.open(n.id)} />
          ))}
        </div>
      )}
    </div>
  );

  if (!reader || !pane.mode) return list;
  return (
    <SplitView baseWidth={baseWidth} closing={pane.closing} savedWidth={savedListWidth} onSaveWidth={setNotesListWidth} list={list} pane={reader} />
  );
}
