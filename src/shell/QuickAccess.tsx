import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LockKeyhole, Power, RotateCw, Settings, ChevronDown, Moon } from "lucide-react";
import { LlmQuickButton } from "./LlmQuickButton";
import { usePanelStore } from "../store";

export function QuickAccess() {
  const setTab = usePanelStore((s) => s.setTab);
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState<"shutdown" | "restart" | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = () => { setOpen(false); setConfirm(null); setError(null); };
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) close(); };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); trigger.current?.focus(); }
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", escape, true);
    return () => { document.removeEventListener("pointerdown", outside); window.removeEventListener("keydown", escape, true); };
  }, [open]);
  const run = async (action: string) => {
    setBusy(true); setError(null);
    try { await invoke("power_action", { action }); close(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const button = "flex min-h-9 items-center justify-center gap-2 rounded-lg px-3 text-[13px] text-fg-muted hover:bg-ink/10 hover:text-fg focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50";
  return (
    <div ref={root} className="relative flex shrink-0 items-center gap-1 border-t border-stroke pt-2" aria-label="Быстрый доступ">
      <button className={button} onClick={() => setTab("settings")} title="Настройки"><Settings className="size-4" /><span>Настройки</span></button>
      <LlmQuickButton onMenuOpen={close} />
      <button ref={trigger} className={`${button} ml-auto`} aria-expanded={open} onClick={() => { if (open) close(); else setOpen(true); }}><Power className="size-4" />Питание<ChevronDown className="size-3.5" /></button>
      {open && (
        <div className="absolute right-0 bottom-full z-40 mb-2 w-72 max-w-full rounded-xl border border-stroke bg-popover p-3 shadow-lg">
          {confirm ? (
            <>
              <p className="text-[14px] font-semibold text-fg">{confirm === "shutdown" ? "Завершить работу?" : "Перезагрузить компьютер?"}</p>
              <p className="mt-1 text-[12px] leading-relaxed text-fg-muted">Сохраните открытые документы перед продолжением.</p>
              <div className="mt-3 flex justify-end gap-1">
                <button className={button} disabled={busy} onClick={() => setConfirm(null)}>Отмена</button>
                <button className={`${button} bg-accent/15`} disabled={busy} onClick={() => void run(confirm)}>{busy ? "Выполняю…" : confirm === "shutdown" ? "Выключить" : "Перезагрузить"}</button>
              </div>
            </>
          ) : (
            <div className="flex flex-col gap-1">
              <button className={`${button} justify-start`} disabled={busy} onClick={() => void run("lock")}><LockKeyhole className="size-4" />Заблокировать компьютер</button>
              <button className={`${button} justify-start`} disabled={busy} onClick={() => void run("sleep")}><Moon className="size-4" />Спящий режим</button>
              <button className={`${button} justify-start`} onClick={() => setConfirm("restart")}><RotateCw className="size-4" />Перезагрузка</button>
              <button className={`${button} justify-start`} onClick={() => setConfirm("shutdown")}><Power className="size-4" />Завершение работы</button>
            </div>
          )}
          {error && <p role="alert" className="mt-2 text-[12px] text-warn">{error}</p>}
        </div>
      )}
      {!open && error && <p role="alert" className="absolute right-0 bottom-full rounded-lg bg-popover p-2 text-[12px] text-warn">{error}</p>}
    </div>
  );
}
