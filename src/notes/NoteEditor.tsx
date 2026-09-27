import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Loader2, Pin, Plus } from "lucide-react";
import clsx from "clsx";
import { AnchoredMenu, menuItem } from "../widgets/spotify/Menu";
import { tagStyle, useNotesActions, type Note, type Tag } from "./api";

/** The pin checkbox of the database, as a button. */
export function PinButton({ note }: { note: Note }) {
  const { setProps } = useNotesActions();
  return (
    <button
      onClick={() => setProps(note.id, { pinned: !note.pinned }).catch(console.error)}
      title={note.pinned ? "Открепить" : "Закрепить"}
      className={clsx(
        "grid size-7 shrink-0 place-items-center rounded-lg hover:bg-ink/10 hover:text-fg",
        note.pinned ? "text-accent" : "text-fg-subtle",
      )}
    >
      {/* Filled while pinned, as in the list. */}
      <Pin className="size-3.5" fill={note.pinned ? "currentColor" : "none"} />
    </button>
  );
}

/** The note's tags, with a menu to add or remove them (and make new ones). */
export function TagEditor({ note, options }: { note: Note; options: Tag[] }) {
  const { setProps } = useNotesActions();
  const [open, setOpen] = useState(false);
  const [fresh, setFresh] = useState("");
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const names = note.tags.map((t) => t.name);
  const set = (tags: string[]) => setProps(note.id, { tags }).catch(console.error);
  const toggle = (name: string) => set(names.includes(name) ? names.filter((n) => n !== name) : [...names, name]);
  const all = [...options, ...note.tags.filter((t) => !options.some((o) => o.name === t.name))];

  return (
    <div className="flex flex-wrap items-center gap-1">
      {note.tags.map((t) => (
        <span key={t.name} style={tagStyle(t.color)} className="rounded px-1.5 text-[11px] leading-5 text-fg-muted">
          {t.name}
        </span>
      ))}
      <button
        ref={anchor}
        onClick={() => setOpen(!open)}
        title="Теги"
        className="flex h-5 items-center gap-0.5 rounded px-1 text-[11px] text-fg-subtle hover:bg-ink/10 hover:text-fg"
      >
        <Plus className="size-3" /> {note.tags.length ? "" : "тег"}
      </button>
      {open && (
        <AnchoredMenu anchor={anchor} onClose={close} className="w-56">
          <div className="max-h-60 overflow-y-auto">
            {all.map((t) => (
              <button key={t.name} className={menuItem} onClick={() => toggle(t.name)}>
                <Check className={clsx("size-3.5 shrink-0", !names.includes(t.name) && "invisible")} />
                <span style={tagStyle(t.color)} className="truncate rounded px-1.5 text-[12px]">
                  {t.name}
                </span>
              </button>
            ))}
          </div>
          <form
            className="mt-1 border-t border-ink/10 p-1"
            onSubmit={(e) => {
              e.preventDefault();
              const name = fresh.trim();
              if (name && !names.includes(name)) set([...names, name]);
              setFresh("");
            }}
          >
            <input
              value={fresh}
              onChange={(e) => setFresh(e.target.value)}
              placeholder="Новый тег, Enter"
              className="h-7 w-full rounded-md border border-stroke bg-field px-2 text-[12px] outline-none focus:border-accent/50"
            />
          </form>
        </AnchoredMenu>
      )}
    </div>
  );
}

/**
 * The note's title and text in edit mode. The text uses the quick-note
 * syntax; on save only what changed goes to Notion, so untouched blocks keep
 * their formatting. ⟦…⟧ lines are blocks the text can't show (images, code).
 */
export function NoteEditor({ note, onDone }: { note: Note; onDone: () => void }) {
  const { text, edit, setProps } = useNotesActions();
  const [title, setTitle] = useState(note.title);
  const [body, setBody] = useState<string | null>(null);
  const [original, setOriginal] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    text(note.id)
      .then((t) => {
        setOriginal(t);
        setBody(t);
      })
      .catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per note
  }, [note.id]);

  const dirty = title !== note.title || (body != null && body !== original);
  const save = async () => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      if (title.trim() !== note.title) await setProps(note.id, { title: title.trim() });
      if (body != null && body !== original) await edit(note, body);
      onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const rows = Math.max(10, (body ?? "").split("\n").length + 2);
  return (
    <div
      className="flex flex-col gap-2.5 pb-4"
      onKeyDown={(e) => {
        if ((e.key === "s" || e.key === "Enter") && e.ctrlKey) {
          e.preventDefault();
          save();
        } else if (e.key === "Escape") {
          e.stopPropagation();
          if (!dirty) onDone();
        }
      }}
    >
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder="Заголовок"
        className="h-10 w-full rounded-lg border border-stroke bg-field px-2.5 text-[16px] font-semibold text-fg outline-none focus:border-accent/50"
      />
      {body == null ? (
        error ? (
          <p className="text-[12.5px] text-warn">{error}</p>
        ) : (
          <p className="flex items-center gap-2 text-[12.5px] text-fg-subtle">
            <Loader2 className="size-4 animate-spin" /> Загружаю текст…
          </p>
        )
      ) : (
        <textarea
          autoFocus
          value={body}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            // Tab indents (nests under the line above) instead of leaving the field.
            if (e.key !== "Tab") return;
            e.preventDefault();
            const el = e.currentTarget;
            const { selectionStart: a, selectionEnd: b, value } = el;
            const lineStart = value.lastIndexOf("\n", a - 1) + 1;
            if (e.shiftKey) {
              if (value.startsWith("  ", lineStart)) {
                setBody(value.slice(0, lineStart) + value.slice(lineStart + 2));
                requestAnimationFrame(() => el.setSelectionRange(Math.max(lineStart, a - 2), Math.max(lineStart, b - 2)));
              }
            } else {
              setBody(value.slice(0, lineStart) + "  " + value.slice(lineStart));
              requestAnimationFrame(() => el.setSelectionRange(a + 2, b + 2));
            }
          }}
          rows={rows}
          spellCheck
          className="w-full resize-none rounded-lg border border-stroke bg-field px-3 py-2.5 font-mono text-[12.5px] leading-relaxed text-fg outline-none focus:border-accent/50"
        />
      )}
      <p className="text-[11px] leading-relaxed text-fg-subtle">
        <code># </code> заголовок · <code>- </code> список · <code>1. </code> нумерация · <code>[ ] </code> чекбокс · <code>&gt; </code> цитата ·{" "}
        <code>+ </code> раскрывающийся · <code>---</code> разделитель · Tab — вложить. Строки ⟦…⟧ — картинки и другие блоки, их можно
        переставить или удалить. Форматирование сохраняется у строк, которые вы не меняли.
      </p>
      {error && body != null && <p className="text-[12px] text-warn">{error}</p>}
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-fg-subtle">Ctrl+S — сохранить{dirty ? "" : ", Esc — выйти"}</span>
        <button onClick={onDone} className="ml-auto rounded-lg px-2.5 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg">
          Отмена
        </button>
        <button
          onClick={save}
          disabled={!dirty || saving || body == null}
          className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1 text-[12px] font-medium text-on-accent disabled:opacity-40"
        >
          {saving && <Loader2 className="size-3.5 animate-spin" />} Сохранить
        </button>
      </div>
    </div>
  );
}
