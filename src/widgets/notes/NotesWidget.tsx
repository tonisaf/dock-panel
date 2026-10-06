import { CloudOff, Pin, Plus, StickyNote } from "lucide-react";
import { WidgetState } from "../../components/WidgetState";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { ago, showPin, useNotes } from "../../notes/api";

const SHOWN = 5;

/** Pinned notes, then the latest ones; a click opens the note in the notes tab. */
export function NotesWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const openNote = usePanelStore((s) => s.openNote);
  const { data: s, error: queryError, isFetching, refetch } = useNotes();
  if (!s) return <Card title="Заметки" icon={StickyNote}><WidgetState kind={queryError ? "error" : "loading"} text={queryError ? String(queryError) : undefined} onRetry={() => void refetch()} retrying={isFetching} /></Card>;

  if (!s.configured) {
    return (
      <Card title="Заметки" icon={StickyNote}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Выберите источник заметок в настройках →
        </button>
      </Card>
    );
  }

  const notes = s.notes.slice(0, SHOWN);
  return (
    <Card
      title="Заметки"
      icon={StickyNote}
      action={
        <button
          onClick={() => openNote("new")}
          title="Новая заметка"
          className="widget-icon-button"
        >
          <Plus className="size-3.5" />
        </button>
      }
    >
      {notes.length === 0 ? (
        <p className="text-[12px] text-fg-subtle">{s.syncing ? "Загружаю заметки…" : "Заметок пока нет"}</p>
      ) : (
        <div className="-mx-1.5 flex flex-col">
          {notes.map((n) => (
            <button key={n.id} onClick={() => openNote(n.id)} className="flex flex-col rounded-lg px-1.5 py-1.5 text-left hover:bg-surface">
              <div className="flex items-center gap-1.5 text-[13px]">
                {n.icon && <span className="shrink-0">{n.icon}</span>}
                <span className="min-w-0 flex-1 truncate">{n.title || "Без названия"}</span>
                {n.local && <CloudOff className="size-3 shrink-0 text-fg-subtle" />}
                {showPin(n) && <Pin className="size-3 shrink-0 text-fg-subtle" />}
                <span className="shrink-0 text-[11px] text-fg-subtle">{ago(n.edited)}</span>
              </div>
              {n.preview && <div className="truncate text-[11.5px] text-fg-subtle">{n.preview}</div>}
            </button>
          ))}
        </div>
      )}
      {s.error && !s.syncing && (
        <p className="mt-1.5 flex items-center gap-1 text-[11px] text-fg-subtle" title={s.error}>
          <CloudOff className="size-3" /> Без связи с Notion — из кэша
        </p>
      )}
    </Card>
  );
}
