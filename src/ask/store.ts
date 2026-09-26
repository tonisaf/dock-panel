import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Model = "haiku" | "sonnet" | "opus";
export type Status = "idle" | "running" | "done" | "error";

const MODEL_KEY = "ask.model";

function savedModel(): Model {
  try {
    const m = localStorage.getItem(MODEL_KEY);
    if (m === "haiku" || m === "sonnet" || m === "opus") return m;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return "sonnet";
}

interface AskState {
  draft: string;
  model: Model;
  /** The question the answer belongs to. */
  question: string;
  answer: string;
  status: Status;
  durationMs: number | null;
  id: number | null;
  setDraft: (draft: string) => void;
  setModel: (model: Model) => void;
  ask: (prompt: string, question?: string) => Promise<void>;
  cancel: () => void;
  clear: () => void;
}

/**
 * One quick question at a time, shared by the AI tab and the widget: asking
 * in one shows the answer in both.
 */
export const useAsk = create<AskState>((set, get) => ({
  draft: "",
  model: savedModel(),
  question: "",
  answer: "",
  status: "idle",
  durationMs: null,
  id: null,
  setDraft: (draft) => set({ draft }),
  setModel: (model) => {
    set({ model });
    try {
      localStorage.setItem(MODEL_KEY, model);
    } catch {
      // Not remembered this time.
    }
  },
  ask: async (prompt, question) => {
    set({ question: question ?? prompt, answer: "", status: "running", durationMs: null, id: null });
    try {
      const id = await invoke<number>("ask_start", { prompt, model: get().model });
      set({ id });
    } catch (e) {
      set({ answer: String(e), status: "error" });
    }
  },
  cancel: () => {
    invoke("ask_cancel").catch(console.error);
    set({ status: get().answer ? "done" : "idle", id: null });
  },
  clear: () => set({ question: "", answer: "", status: "idle", durationMs: null, id: null }),
}));

// Events carry the id of the question they belong to; stale ones are ignored.
listen<{ id: number; text: string }>("ask:delta", (e) => {
  const s = useAsk.getState();
  if (s.status === "running" && (s.id === null || s.id === e.payload.id)) {
    useAsk.setState({ answer: s.answer + e.payload.text, id: e.payload.id });
  }
}).catch(console.error);

listen<{ id: number; text: string; error: boolean; durationMs: number }>("ask:done", (e) => {
  const s = useAsk.getState();
  if (s.status === "running" && (s.id === null || s.id === e.payload.id)) {
    useAsk.setState({
      answer: e.payload.text,
      status: e.payload.error ? "error" : "done",
      durationMs: e.payload.durationMs,
      id: null,
    });
  }
}).catch(console.error);

/** Ready-made requests around the clipboard's text. */
export const TEMPLATES: { id: string; label: string; build: (text: string) => string }[] = [
  {
    id: "explain",
    label: "Объясни код",
    build: (t) => `Объясни, что делает этот код, простыми словами. Если видишь ошибки — укажи.\n\n\`\`\`\n${t}\n\`\`\``,
  },
  {
    id: "translate",
    label: "Переведи",
    build: (t) =>
      `Переведи текст: если он на русском — на английский, иначе — на русский. Ответь только переводом.\n\n${t}`,
  },
  {
    id: "fix",
    label: "Исправь текст",
    build: (t) =>
      `Исправь орфографию, пунктуацию и стиль, сохранив смысл и язык. Ответь только исправленным текстом.\n\n${t}`,
  },
  {
    id: "summary",
    label: "Кратко",
    build: (t) => `Перескажи кратко, в 3–5 пунктах.\n\n${t}`,
  },
];

/** Runs a template on the clipboard's text; throws when the clipboard has none. */
export async function askTemplate(id: string) {
  const template = TEMPLATES.find((t) => t.id === id);
  if (!template) return;
  const text = await invoke<string | null>("clipboard_text");
  if (!text) throw new Error("В буфере обмена нет текста");
  const preview = text.length > 80 ? `${text.slice(0, 80).replace(/\s+/g, " ")}…` : text.replace(/\s+/g, " ");
  await useAsk.getState().ask(template.build(text), `${template.label}: ${preview}`);
}
