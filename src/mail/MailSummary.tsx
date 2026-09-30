import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, Sparkles } from "lucide-react";
import type { Letter } from "./api";

/** Summaries made this session, so reopening a letter does not ask the model again. */
const cache = new Map<string, string>();

/** A short summary of the open letter from the local LM Studio model, on request. */
export function MailSummary({ letter }: { letter: Letter }) {
  const key = `${letter.account}/${letter.uid}`;
  const [summary, setSummary] = useState<string | null>(cache.get(key) ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const text = await invoke<string>("llm_run", {
        task: "mail",
        text: `От: ${letter.fromName} <${letter.fromEmail}>\nТема: ${letter.subject}\n\n${letter.text}`,
      });
      cache.set(key, text);
      setSummary(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!summary) {
    return (
      <div className="mt-3 flex flex-col items-start gap-1.5">
        <button
          onClick={run}
          disabled={busy || !letter.text.trim()}
          className="flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50"
        >
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}
          {busy ? "LM Studio читает…" : "Кратко"}
        </button>
        {error && <p className="text-[12px] text-warn">{error}</p>}
      </div>
    );
  }
  return (
    <div className="mt-3 rounded-xl bg-accent/10 px-3 py-2.5">
      <div className="mb-1 flex items-center gap-1.5 text-[11px] font-medium tracking-wide text-accent uppercase">
        <Sparkles className="size-3" /> Кратко
      </div>
      <p className="text-[13px] leading-relaxed whitespace-pre-wrap select-text">{summary}</p>
    </div>
  );
}
