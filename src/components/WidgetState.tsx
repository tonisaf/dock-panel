import { Loader2, RefreshCw } from "lucide-react";

/** Shared feedback for widget data; retry only repeats a read. */
export function WidgetState({ kind, text, onRetry, retrying = false }: {
  kind: "loading" | "empty" | "error";
  text?: string;
  onRetry?: () => void;
  retrying?: boolean;
}) {
  return (
    <div role={kind === "error" ? "alert" : "status"} className="flex min-h-16 flex-col justify-center gap-2 py-2 text-[13px] leading-relaxed">
      <p className={`flex items-start gap-2 ${kind === "error" ? "text-warn" : "text-fg-muted"}`}>
        {kind === "loading" && <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin" />}
        <span className="min-w-0 break-words">{text ?? (kind === "loading" ? "Загрузка…" : kind === "empty" ? "Пока нет данных" : "Не удалось загрузить данные")}</span>
      </p>
      {kind === "error" && onRetry && (
        <button type="button" onClick={onRetry} disabled={retrying} className="flex min-h-8 self-start items-center gap-1.5 rounded-lg border border-stroke px-3 text-[12px] text-fg-muted hover:bg-ink/10 hover:text-fg focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50">
          <RefreshCw className={`size-3.5 ${retrying ? "animate-spin" : ""}`} />
          {retrying ? "Повторяю…" : "Повторить"}
        </button>
      )}
    </div>
  );
}
