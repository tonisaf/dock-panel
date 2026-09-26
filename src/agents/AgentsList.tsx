import { useState } from "react";
import { Bell, BellOff, Check, Loader2 } from "lucide-react";
import clsx from "clsx";
import { ago, useAgentActions, useAgents, type Agent } from "./api";

function StatusMark({ agent }: { agent: Agent }) {
  if (agent.busy) return <Loader2 className="size-3.5 shrink-0 animate-spin text-accent" />;
  return (
    <span
      className={clsx("mx-[3px] size-2 shrink-0 rounded-full", agent.waiting ? "bg-accent" : "bg-ink/20")}
      aria-hidden
    />
  );
}

function AgentRow({ agent, compact }: { agent: Agent; compact: boolean }) {
  const { focus, dismiss } = useAgentActions();
  const [error, setError] = useState<string | null>(null);
  const status = agent.busy ? "работает" : agent.waiting ? "ждёт вас" : "свободен";
  return (
    <div
      role="button"
      onClick={() => {
        setError(null);
        focus(agent.id).catch((e) => setError(String(e)));
      }}
      title="Открыть окно сессии"
      className="group flex cursor-default items-start gap-2.5 rounded-xl px-2 py-1.5 hover:bg-ink/5"
    >
      <div className="grid h-5 place-items-center">
        <StatusMark agent={agent} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className={clsx("truncate text-[13.5px]", agent.waiting && "font-medium")}>{agent.name}</span>
        </div>
        <div className="truncate text-[11.5px] text-fg-subtle">
          {agent.kind === "claude" ? "Claude" : "Codex"} · {agent.project} · {agent.host} ·{" "}
          <span className={clsx(agent.waiting && "text-accent")}>{status}</span> {ago(agent.since)}
        </div>
        {!compact && agent.waiting && agent.lastMessage && (
          <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-fg-muted">{agent.lastMessage}</p>
        )}
        {error && <p className="text-[11.5px] text-warn">{error}</p>}
      </div>
      {agent.waiting && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            dismiss(agent.id).catch(console.error);
          }}
          title="Отметить просмотренной"
          className="grid size-6 shrink-0 place-items-center rounded-md text-fg-subtle opacity-0 group-hover:opacity-100 hover:bg-ink/10 hover:text-fg"
        >
          <Check className="size-3.5" />
        </button>
      )}
    </div>
  );
}

/** Bell toggle for "a session finished" toasts. */
export function AgentsNotifyToggle() {
  const { data } = useAgents();
  const { setNotify } = useAgentActions();
  const on = data?.notify ?? true;
  return (
    <button
      onClick={() => setNotify(!on).catch(console.error)}
      aria-pressed={on}
      title={on ? "Уведомлять, когда сессия закончила: вкл" : "Уведомлять, когда сессия закончила"}
      className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
    >
      {on ? <Bell className="size-3.5" /> : <BellOff className="size-3.5" />}
    </button>
  );
}

/**
 * Claude Code and Codex sessions: working ones first, then those waiting for
 * the user. A click brings the session's window to the front.
 */
export function AgentsList({ compact = false }: { compact?: boolean }) {
  const { data, isPending } = useAgents();
  const { dismiss } = useAgentActions();
  if (isPending || !data) return null;
  const agents = data.agents;
  const waiting = agents.filter((a) => a.waiting).length;
  if (agents.length === 0) {
    return (
      <p className="text-[12px] leading-relaxed text-fg-subtle">
        Сейчас нет запущенных сессий Claude Code или Codex. Они появятся здесь сами.
      </p>
    );
  }
  return (
    <div className="-mx-2 flex flex-col">
      {agents.map((a) => (
        <AgentRow key={a.id} agent={a} compact={compact} />
      ))}
      {waiting > 1 && (
        <button
          onClick={() => dismiss(null).catch(console.error)}
          className="mt-1 self-start rounded-md px-2 py-0.5 text-[11.5px] text-fg-subtle hover:bg-ink/10 hover:text-fg"
        >
          Отметить все просмотренными
        </button>
      )}
    </div>
  );
}
