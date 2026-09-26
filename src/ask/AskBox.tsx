import { useState } from "react";
import { ArrowUp, Check, Copy, Loader2, Square, X } from "lucide-react";
import clsx from "clsx";
import { Markdown } from "./Markdown";
import { askTemplate, TEMPLATES, useAsk, type Model } from "./store";

const MODELS: { value: Model; label: string; hint: string }[] = [
  { value: "haiku", label: "Haiku", hint: "Быстро" },
  { value: "sonnet", label: "Sonnet", hint: "Обычно" },
  { value: "opus", label: "Opus", hint: "Умнее, дольше" },
];

/**
 * A question box for Claude through Claude Code (`claude -p`): free text or
 * a template on the clipboard's text, with the answer streaming in below.
 * `compact` trims it for the home widget.
 */
export function AskBox({ compact = false }: { compact?: boolean }) {
  const { draft, setDraft, model, setModel, question, answer, status, durationMs, ask, cancel, clear } = useAsk();
  const [templateError, setTemplateError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const running = status === "running";

  const send = () => {
    const prompt = draft.trim();
    if (!prompt || running) return;
    setTemplateError(null);
    setDraft("");
    ask(prompt).catch(console.error);
  };

  const copy = () => {
    navigator.clipboard
      .writeText(answer)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(console.error);
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-end gap-1.5 rounded-xl border border-stroke bg-field px-2.5 py-1.5 focus-within:border-accent/50">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends; Shift+Enter is a new line. Esc stays with the panel.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          rows={Math.min(5, Math.max(1, draft.split("\n").length))}
          placeholder="Спросить Claude…"
          className="max-h-32 min-h-6 flex-1 resize-none bg-transparent py-0.5 text-[13px] leading-relaxed outline-none placeholder:text-fg-subtle"
        />
        {running ? (
          <button
            onClick={cancel}
            title="Остановить"
            className="grid size-7 shrink-0 place-items-center rounded-lg bg-ink/10 text-fg hover:bg-ink/15"
          >
            <Square className="size-3" fill="currentColor" />
          </button>
        ) : (
          <button
            onClick={send}
            disabled={!draft.trim()}
            title="Спросить (Enter)"
            className="grid size-7 shrink-0 place-items-center rounded-lg bg-accent text-white disabled:opacity-30"
          >
            <ArrowUp className="size-4" />
          </button>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-1">
        {TEMPLATES.map((t) => (
          <button
            key={t.id}
            disabled={running}
            onClick={() => {
              setTemplateError(null);
              askTemplate(t.id).catch((e) => setTemplateError(e instanceof Error ? e.message : String(e)));
            }}
            title={`${t.label} — текст из буфера обмена`}
            className="rounded-md bg-ink/8 px-2 py-0.5 text-[11.5px] text-fg-muted hover:bg-ink/12 hover:text-fg disabled:opacity-40"
          >
            {t.label}
          </button>
        ))}
        {!compact && (
          <div className="ml-auto flex rounded-md border border-stroke p-0.5">
            {MODELS.map((m) => (
              <button
                key={m.value}
                onClick={() => setModel(m.value)}
                title={m.hint}
                className={clsx(
                  "rounded px-1.5 py-px text-[11px] transition-colors",
                  model === m.value ? "bg-ink/10 text-fg" : "text-fg-subtle hover:text-fg",
                )}
              >
                {m.label}
              </button>
            ))}
          </div>
        )}
      </div>
      {templateError && <p className="text-[11.5px] text-warn">{templateError}</p>}

      {status !== "idle" && (
        <div className="flex flex-col gap-1.5 rounded-xl bg-ink/5 px-3 py-2.5">
          <div className="flex items-start gap-2">
            <p className="line-clamp-2 flex-1 text-[11.5px] text-fg-subtle">{question}</p>
            {!running && (
              <button onClick={clear} title="Убрать ответ" className="text-fg-subtle hover:text-fg">
                <X className="size-3.5" />
              </button>
            )}
          </div>
          {status === "error" ? (
            <p className="text-[12.5px] leading-relaxed text-warn select-text">{answer}</p>
          ) : answer ? (
            <div className={clsx(compact && "max-h-72 overflow-y-auto")}>
              <Markdown text={answer} />
            </div>
          ) : (
            <p className="flex items-center gap-2 text-[12px] text-fg-subtle">
              <Loader2 className="size-3.5 animate-spin" /> Claude думает…
            </p>
          )}
          {status === "done" && (
            <div className="flex items-center gap-2 text-[11px] text-fg-subtle">
              <button onClick={copy} className="flex items-center gap-1 hover:text-fg">
                {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
                {copied ? "Скопировано" : "Копировать"}
              </button>
              {durationMs != null && <span className="ml-auto tabular-nums">{(durationMs / 1000).toFixed(1)} с</span>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
