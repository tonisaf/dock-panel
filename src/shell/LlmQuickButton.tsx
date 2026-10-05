import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { Check, Cpu, Loader2, Power, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { usePrefs } from "../lib/prefs";
import { useLmStudioOverview } from "../widgets/lmstudio/LmStudioWidget";

export function LlmQuickButton({ onMenuOpen }: { onMenuOpen: () => void }) {
  const { data, error: queryError, refetch } = useLmStudioOverview();
  const queryClient = useQueryClient();
  const preferred = usePrefs((s) => s.quickLlmModel);
  const select = usePrefs((s) => s.setQuickLlmModel);
  const [menu, setMenu] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const models = [...(data?.loaded ?? []), ...(data?.available ?? [])]
    .filter((m, i, all) => m.kind !== "embedding" && all.findIndex((other) => other.key === m.key) === i);
  const model = models.find((m) => m.key === preferred)
    ?? data?.loaded.find((m) => m.kind !== "embedding") ?? models[0];
  const loaded = model && data?.loaded.find((m) => m.key === model.key);
  useEffect(() => {
    if (data && !data.error && model && preferred !== model.key) select(model.key);
  }, [data, model?.key, preferred, select]);
  const open = () => { onMenuOpen(); setMenu(true); };
  useEffect(() => {
    if (!menu) return;
    root.current?.querySelector<HTMLButtonElement>("div.absolute button:not(:disabled)")?.focus();
    const outside = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) setMenu(false); };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setMenu(false); trigger.current?.focus(); }
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", escape, true);
    return () => { document.removeEventListener("pointerdown", outside); window.removeEventListener("keydown", escape, true); };
  }, [menu]);
  const run = async (command: string, args: Record<string, unknown>) => {
    if (busy) return;
    setBusy(true); setError("");
    try { await invoke(command, args); }
    catch (e) { setError(String(e)); open(); }
    finally {
      setBusy(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["lmstudio-overview"] }),
        queryClient.invalidateQueries({ queryKey: ["lmstudio-status"] }),
      ]);
    }
  };
  const toggle = () => {
    if (!model || !data || data.error || queryError || (!loaded && !data.running)) { open(); return; }
    if (loaded) {
      // Never pass null: that command unloads every model, not just this one.
      if (!loaded.identifier) { setError("У модели нет идентификатора для безопасной выгрузки"); open(); return; }
      void run("lmstudio_unload", { identifier: loaded.identifier });
    } else {
      let ttl = 0;
      try { ttl = Number(localStorage.getItem("lmstudio.ttl")) || 0; } catch { /* use default */ }
      void run("lmstudio_load", { key: model.key, ttlMinutes: [10, 30, 60].includes(ttl) ? ttl : null });
    }
  };
  const item = "flex min-h-9 w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-50";
  return (
    <div className="relative" ref={root}>
      <button ref={trigger} type="button" disabled={busy} onClick={toggle}
        onContextMenu={(e) => { e.preventDefault(); open(); }}
        onKeyDown={(e) => { if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); open(); } }}
        aria-pressed={!!loaded} aria-expanded={menu}
        title={`${model?.name ?? "Выберите модель"} · ${loaded ? "Выгрузить" : "Загрузить"}; правый клик — модели и сервер`}
        className={clsx("flex min-h-9 items-center gap-2 rounded-lg px-3 text-[13px] hover:bg-ink/10 focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50", loaded ? "text-accent bg-accent/10" : "text-fg-muted")}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : <Cpu className="size-4" />} LLM
        <span className={clsx("size-1.5 rounded-full", data?.running ? "bg-ok" : "bg-ink/25")} title={data?.running ? "Сервер запущен" : "Сервер остановлен"} />
      </button>
      {menu && <div className="absolute bottom-full left-0 z-40 mb-2 w-80 max-w-[calc(100vw-2rem)] rounded-xl border border-stroke bg-popover p-2 shadow-lg">
        <button className={item} disabled={busy || !data} onClick={() => void run("lmstudio_server", { start: !data?.running })}>
          <Power className="size-4" />{data?.running ? "Остановить сервер" : "Запустить сервер"}
        </button>
        <p className="border-t border-stroke px-2.5 pt-2 text-[12px] text-fg-subtle">Модель для кнопки LLM</p>
        <div className="max-h-[40vh] overflow-y-auto">
          {models.map((m) => <button key={m.key} disabled={busy} className={item} onClick={() => { select(m.key); setMenu(false); }}>
            <span className="grid size-4 shrink-0 place-items-center">{m.key === model?.key && <Check className="size-4" />}</span>
            <span className="min-w-0 truncate" title={m.name}>{m.name}</span>
            {data?.loaded.some((loadedModel) => loadedModel.key === m.key) && <span className="ml-auto shrink-0 text-[11px] text-accent">загружена</span>}
          </button>)}
        </div>
        {!models.length && <p className="px-2.5 py-2 text-[12px] text-fg-subtle">Нет доступных моделей. Проверьте LM Studio.</p>}
        {preferred && !model && <p className="px-2.5 py-1 text-[12px] text-warn">Выбранная модель недоступна. Выберите другую.</p>}
        {(error || data?.error || queryError) && <p role="alert" className="px-2.5 py-1 text-[12px] text-warn">{error || data?.error || String(queryError)}</p>}
        <button className={item} disabled={busy} onClick={() => void refetch()}><RefreshCw className="size-4" />Обновить список</button>
      </div>}
    </div>
  );
}
