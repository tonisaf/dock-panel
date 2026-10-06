import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { saveIntegrations, useIntegrations, tasksProvider, type Integrations } from "../lib/integrations";
const field = "h-9 rounded-lg border border-stroke bg-field px-2 text-[13px] text-fg";
export function IntegrationSettings({ kind }: { kind: "llm" | "notes" }) {
  const saved = useIntegrations();
  const [draft, setDraft] = useState<Integrations | null>(null);
  const s = draft ?? saved;
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const queryClient = useQueryClient();
  const save = async () => {
    setBusy(true); setError("");
    try {
      await queryClient.cancelQueries();
      await saveIntegrations({ ...saved, [kind]: s[kind], ...(kind === "notes" ? { vault: s.vault, tasks: s.tasks } : {}) });
      setDraft(null);
      await queryClient.invalidateQueries();
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <div className="flex flex-col gap-2 p-3.5 text-[12.5px] text-fg-muted">
    <label className="flex items-center gap-2">{kind === "llm" ? "Локальная модель" : "Источник заметок"}
      <select className={field + " flex-1"} value={s[kind]} onChange={(e) => setDraft({ ...s, [kind]: e.target.value, ...(kind === "notes" && e.target.value === "local" ? { tasks: tasksProvider(s) } : {}) })}>
        {kind === "llm" ? <><option value="lmstudio">LM Studio</option><option value="ollama">Ollama</option></> : <><option value="notion">Notion</option><option value="obsidian">Obsidian</option><option value="local">Заметки Dock Panel</option></>}
      </select>
    </label>
    {kind === "notes" && s.notes === "local" && <p>Собственная доска заметок. Хранятся локально в SQLite; списки остаются внутри заметок. Для задач можно сохранить Notion или Obsidian.</p>}
    {kind === "notes" && s.notes === "local" && <label className="flex items-center gap-2">Источник задач<select className={field + " flex-1"} value={tasksProvider(s)} onChange={(e) => setDraft({ ...s, tasks: e.target.value as Integrations["tasks"] })}><option value="notion">Notion</option><option value="obsidian">Obsidian</option></select></label>}
    {kind === "notes" && (s.notes === "obsidian" || (s.notes === "local" && tasksProvider(s) === "obsidian")) && <>
      <label className="flex flex-col gap-1">Папка хранилища Obsidian<input className={field} value={s.vault} placeholder="C:\\Users\\…\\Мои заметки" onChange={(e) => setDraft({ ...s, vault: e.target.value })} /></label>
      <p>Заметки — Markdown-файлы; задачи — строки «- [ ]». Новые записи сохраняются в папке Dock Panel. Расширения Obsidian и поля задач Notion не переносятся.</p>
    </>}
    {kind === "llm" && s.llm === "ollama" && <p>Ollama на 127.0.0.1:11434. Сначала установите модель командой «ollama pull имя». Панель умеет загрузить и выгрузить установленные модели; внешним сервером управляйте через трей Ollama.</p>}
    {draft && <button disabled={busy} className="self-start rounded-lg border border-stroke px-3 py-1.5 hover:bg-ink/8 disabled:opacity-50" onClick={() => void save()}>{busy ? "Сохраняю…" : "Сохранить выбор"}</button>}
    {error && <p role="alert" className="text-warn">{error}</p>}
  </div>;
}
