import { useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import clsx from "clsx";
import { useUpdateActions, useUpdateStatus } from "../lib/updates";

const button =
  "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";

export function UpdateSettings() {
  const { data: status } = useUpdateStatus();
  const { check, install, installing, progress, error } = useUpdateActions();
  const [checking, setChecking] = useState(false);

  if (!status) return null;

  const runCheck = async () => {
    setChecking(true);
    await check().finally(() => setChecking(false));
  };

  const message = error ?? status.error;

  return (
    <div className="flex flex-col gap-2 px-3.5 py-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[14px]">Версия {status.current}</div>
          <div className={clsx("text-[12px]", status.available ? "text-accent" : "text-fg-subtle")}>
            {status.available ? `Доступна ${status.available.version}` : "Проверка раз в 6 часов"}
          </div>
        </div>
        {status.available ? (
          <button className={button} disabled={installing} onClick={install}>
            {installing && <Loader2 className="size-3.5 animate-spin" />}
            {installing ? (progress != null ? `${progress}%` : "Скачиваю…") : "Обновить"}
          </button>
        ) : (
          <button className={button} disabled={checking} onClick={runCheck}>
            <RefreshCw className={clsx("size-3.5", checking && "animate-spin")} /> Проверить
          </button>
        )}
      </div>
      {status.available?.notes && (
        <p className="text-[12px] leading-relaxed whitespace-pre-line text-fg-muted">{status.available.notes}</p>
      )}
      {message && <p className="text-[12px] leading-relaxed text-warn">{message}</p>}
    </div>
  );
}
