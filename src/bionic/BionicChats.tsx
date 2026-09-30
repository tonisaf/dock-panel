import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ChevronRight, Wrench } from "lucide-react";
import clsx from "clsx";
import { ago } from "../agents/api";
import { Markdown } from "../ask/Markdown";

interface Session {
  id: string;
  title: string;
  updatedMs: number;
  unread: boolean;
  model: string | null;
}

interface Project {
  id: string;
  name: string;
  sessions: Session[];
}

interface Block {
  kind: "text" | "reasoning" | "tool" | "result";
  text: string;
}

interface Message {
  id: string;
  role: "user" | "assistant" | "tool";
  ts: number | null;
  blocks: Block[];
}

function useProjects() {
  return useQuery({
    queryKey: ["bionic-projects"],
    queryFn: () => invoke<Project[]>("bionic_projects"),
    staleTime: 15_000,
    refetchOnWindowFocus: true,
  });
}

function useMessages(projectId: string, sessionId: string) {
  return useQuery({
    queryKey: ["bionic-session", projectId, sessionId],
    queryFn: () => invoke<Message[]>("bionic_session", { projectId, sessionId }),
    staleTime: 15_000,
  });
}

function BlockView({ block }: { block: Block }) {
  if (block.kind === "text") return <Markdown text={block.text} />;
  if (block.kind === "reasoning") {
    return (
      <details className="text-[12px] text-fg-subtle">
        <summary className="cursor-default select-none">Размышления</summary>
        <p className="mt-1 whitespace-pre-wrap leading-snug">{block.text}</p>
      </details>
    );
  }
  if (block.kind === "tool") {
    return (
      <p className="flex items-start gap-1.5 font-mono text-[11.5px] break-all text-fg-muted">
        <Wrench className="mt-0.5 size-3 shrink-0" /> {block.text}
      </p>
    );
  }
  return (
    <details className="text-[11.5px] text-fg-subtle">
      <summary className="cursor-default select-none">Результат</summary>
      <pre className="mt-1 max-h-48 overflow-auto font-mono break-all whitespace-pre-wrap">{block.text}</pre>
    </details>
  );
}

function Chat({ projectId, session, onBack }: { projectId: string; session: Session; onBack: () => void }) {
  const { data, isPending, error } = useMessages(projectId, session.id);
  return (
    <div className="flex flex-col gap-2">
      <button
        onClick={onBack}
        className="flex items-center gap-1.5 self-start rounded-lg px-1.5 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg"
      >
        <ArrowLeft className="size-3.5" /> <span className="truncate">{session.title}</span>
      </button>
      {isPending ? (
        <p className="text-[12px] text-fg-subtle">Загружаю…</p>
      ) : error ? (
        <p className="text-[12px] text-warn">{String(error)}</p>
      ) : data.length === 0 ? (
        <p className="text-[12px] text-fg-subtle">В чате нет сообщений.</p>
      ) : (
        <div className="flex max-h-[28rem] flex-col gap-2.5 overflow-y-auto pr-1">
          {data.map((m) => (
            <div
              key={m.id}
              className={clsx(
                "flex flex-col gap-1.5 rounded-xl px-2.5 py-2 text-[13px] leading-relaxed",
                m.role === "user" ? "bg-accent/10" : m.role === "tool" ? "bg-ink/5" : "bg-ink/[0.03]",
              )}
            >
              {m.blocks.map((b, i) => (
                <BlockView key={i} block={b} />
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Bionic (LM Studio) projects and their chats, read-only. */
export function BionicChats() {
  const { data: projects, isPending } = useProjects();
  const [open, setOpen] = useState<{ projectId: string; session: Session } | null>(null);

  if (open) return <Chat projectId={open.projectId} session={open.session} onBack={() => setOpen(null)} />;
  if (isPending) return null;
  if (!projects?.length) {
    return <p className="text-[12px] text-fg-subtle">Чатов Bionic пока нет.</p>;
  }
  return (
    <div className="flex flex-col gap-2.5">
      {projects.map((p) => (
        <div key={p.id}>
          <div className="px-2 pb-0.5 text-[11px] font-medium tracking-wide text-fg-subtle uppercase">{p.name}</div>
          {p.sessions.map((s) => (
            <div
              key={s.id}
              role="button"
              onClick={() => setOpen({ projectId: p.id, session: s })}
              className="flex cursor-default items-center gap-2 rounded-xl px-2 py-1.5 hover:bg-ink/5"
            >
              <div className="min-w-0 flex-1">
                <div className={clsx("truncate text-[13.5px]", s.unread && "font-medium")}>{s.title}</div>
                <div className="truncate text-[11.5px] text-fg-subtle">
                  {s.model ? `${s.model} · ` : ""}
                  {ago(s.updatedMs)}
                </div>
              </div>
              <ChevronRight className="size-3.5 shrink-0 text-fg-subtle" />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
