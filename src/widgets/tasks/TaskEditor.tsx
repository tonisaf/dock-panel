import { useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, Trash2, X } from "lucide-react";
import clsx from "clsx";
import { usePanelStore } from "../../store";
import { notionColor, useTaskActions, useTaskSchema, type Badge, type Task, type TaskChange } from "./api";

const field =
  "h-8 w-full min-w-0 rounded-lg border border-stroke bg-field px-2 text-[12.5px] text-fg outline-none focus:border-accent/50";

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span className="text-[11px] text-fg-subtle">{label}</span>
      {children}
    </label>
  );
}

function Select({ value, options, empty, onChange }: { value: string; options: Badge[]; empty?: string; onChange: (v: string) => void }) {
  const color = options.find((o) => o.name === value)?.color;
  return (
    <select value={value} onChange={(e) => onChange(e.target.value)} className={field} style={color ? { color: notionColor(color) } : undefined}>
      {empty != null && <option value="">{empty}</option>}
      {options.map((o) => (
        <option key={o.name} value={o.name}>
          {o.name}
        </option>
      ))}
    </select>
  );
}

/** A task's fields, edited in place; each change goes to Notion at once. */
export function TaskEditor({ task, onClose }: { task: Task; onClose: () => void }) {
  const { data: schema, isPending } = useTaskSchema(true);
  const { update, remove } = useTaskActions();
  const [title, setTitle] = useState(task.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const save = async (change: TaskChange) => {
    setBusy(true);
    setError(null);
    try {
      await update(task, change);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const saveTitle = () => {
    const t = title.trim();
    if (t && t !== task.title) save({ title: t });
    else setTitle(task.title);
  };
  const del = async () => {
    if (!confirmDelete) return setConfirmDelete(true);
    try {
      await remove(task);
    } catch (e) {
      setError(String(e));
      setConfirmDelete(false);
    }
  };

  return (
    <div
      className="flex flex-col gap-2.5 rounded-xl border border-stroke bg-surface p-2.5"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="flex items-center gap-1.5">
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={saveTitle}
          onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
          placeholder="Название"
          className={clsx(field, "h-9 text-[13.5px] font-medium")}
        />
        <button onClick={onClose} title="Свернуть (Esc)" className="grid size-8 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg">
          <X className="size-4" />
        </button>
      </div>

      {isPending ? (
        <p className="flex items-center gap-2 text-[12px] text-fg-subtle">
          <Loader2 className="size-3.5 animate-spin" /> Загружаю поля базы…
        </p>
      ) : (
        schema && (
          <div className="grid grid-cols-2 gap-2">
            {schema.hasStatus && (
              <Field label="Статус">
                <Select value={task.status?.name ?? ""} options={schema.statuses} empty={task.status ? undefined : "—"} onChange={(v) => v && save({ status: v })} />
              </Field>
            )}
            {schema.hasDue && (
              <Field label="Срок">
                <div className="flex gap-1">
                  <input
                    type="date"
                    value={task.due?.slice(0, 10) ?? ""}
                    onChange={(e) => save({ due: e.target.value || null })}
                    className={field}
                  />
                  {task.due && (
                    <button onClick={() => save({ due: null })} title="Убрать срок" className="grid size-8 shrink-0 place-items-center rounded-lg text-fg-subtle hover:bg-ink/10 hover:text-fg">
                      <X className="size-3.5" />
                    </button>
                  )}
                </div>
              </Field>
            )}
            {schema.hasPriority && (
              <Field label="Приоритет">
                <Select value={task.priority?.name ?? ""} options={schema.priorities} empty="—" onChange={(v) => save({ priority: v || null })} />
              </Field>
            )}
            {schema.hasTag && (
              <Field label="Область">
                <Select value={task.tag?.name ?? ""} options={schema.tags} empty="—" onChange={(v) => save({ tag: v || null })} />
              </Field>
            )}
          </div>
        )
      )}

      {error && <p className="text-[12px] leading-relaxed text-warn">{error}</p>}

      <div className="flex items-center gap-1.5">
        {task.url && (
          <button
            onClick={() => {
              openUrl(task.url).catch(console.error);
              usePanelStore.getState().setOpen(false);
            }}
            className="flex items-center gap-1.5 rounded-lg px-2 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg"
          >
            <ExternalLink className="size-3.5" /> Открыть в {task.url.startsWith("obsidian:") ? "Obsidian" : "Notion"}
          </button>
        )}
        {busy && <Loader2 className="size-3.5 animate-spin text-fg-subtle" />}
        <button
          onClick={del}
          onBlur={() => setConfirmDelete(false)}
          className={clsx(
            "ml-auto flex items-center gap-1.5 rounded-lg px-2 py-1 text-[12px]",
            confirmDelete ? "bg-red-500/15 text-red-400" : "text-fg-subtle hover:bg-ink/8 hover:text-fg",
          )}
        >
          <Trash2 className="size-3.5" /> {confirmDelete ? "Точно удалить?" : "Удалить"}
        </button>
      </div>
    </div>
  );
}
