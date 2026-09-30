import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { Bell, BellOff, Clock, LogIn, LogOut, MessageCircleQuestion, MessagesSquare, RefreshCw, Sparkle, Unlink } from "lucide-react";
import { AskBox } from "../ask/AskBox";
import clsx from "clsx";
import { Card } from "../components/Card";
import { BionicChats } from "../bionic/BionicChats";
import { AgentsList, AgentsNotifyToggle } from "../agents/AgentsList";
import { LimitBars } from "../widgets/ai/LimitBars";
import {
  claudePlanLabel,
  effectiveWindows,
  formatAgo,
  formatResetAt,
  useAiLimits,
  windowLabel,
  type AiLimits,
  type Snapshot,
} from "../widgets/ai/api";

function Resets({ snapshot }: { snapshot: Snapshot }) {
  return (
    <div className="mt-3 flex flex-col gap-1 border-t border-stroke pt-3 text-[12px] text-fg-subtle">
      {effectiveWindows(snapshot)
        .filter((w) => !w.reset)
        .map((w) => (
          <div key={w.kind} className="flex justify-between">
            <span>Сброс окна «{windowLabel(w.kind)}»</span>
            <span className="tabular-nums text-fg-muted">{formatResetAt(w.resetsAt)}</span>
          </div>
        ))}
      <div className="flex items-center gap-1.5 pt-1">
        <Clock className="size-3.5" /> Данные обновлены {formatAgo(snapshot.updatedAt)}
      </div>
    </div>
  );
}

function BellToggle({ provider, on }: { provider: "claude" | "codex"; on: boolean }) {
  const queryClient = useQueryClient();
  const [pending, setPending] = useState(false);

  const toggle = async () => {
    setPending(true);
    try {
      await invoke("ai_alerts_set", { provider, enabled: !on });
      await queryClient.invalidateQueries({ queryKey: ["ai-limits"] });
    } finally {
      setPending(false);
    }
  };

  return (
    <button
      onClick={toggle}
      disabled={pending}
      aria-pressed={on}
      title={on ? "Уведомлять о сбросе лимитов: вкл" : "Уведомлять о сбросе лимитов"}
      className={clsx(
        "grid size-7 place-items-center rounded-lg border transition-colors disabled:opacity-50",
        on ? "border-accent/40 bg-accent/15 text-accent" : "border-stroke text-fg-subtle hover:bg-ink/8 hover:text-fg",
      )}
    >
      {on ? <Bell className="size-3.5" fill="currentColor" /> : <BellOff className="size-3.5" />}
    </button>
  );
}

function ClaudeCard({ data }: { data: AiLimits }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { claude: snapshot, claudeWeb: web, claudeConnected } = data;

  const run = async (command: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke(command);
      await queryClient.invalidateQueries({ queryKey: ["ai-limits"] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const button =
    "flex items-center gap-1.5 rounded-lg border border-stroke px-2.5 py-1.5 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg disabled:opacity-50";
  const plan = claudePlanLabel(snapshot?.plan ?? null);

  return (
    <Card>
      <div className="mb-3 flex items-center justify-between gap-2">
        <span className="text-[15px] font-semibold">Claude</span>
        <div className="flex items-center gap-2">
          {web.enabled && (
            <span className="rounded-md border border-warn/30 px-1.5 py-0.5 text-[10px] text-warn/80 uppercase">
              эксперимент
            </span>
          )}
          {plan && (
            <span className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-muted">{plan}</span>
          )}
          <BellToggle provider="claude" on={data.alerts.claude} />
        </div>
      </div>

      {web.enabled && web.needsLogin ? (
        <p className="text-[12px] leading-relaxed text-fg-muted">
          Нужно войти в claude.ai: сессия не найдена или истекла.
        </p>
      ) : snapshot ? (
        <>
          <LimitBars snapshot={snapshot} large />
          <Resets snapshot={snapshot} />
        </>
      ) : web.enabled ? (
        <p className="text-[12px] leading-relaxed text-fg-muted">
          {web.fetching ? "Загружаю данные с claude.ai…" : "Данных пока нет. Нажмите «Обновить» после входа."}
        </p>
      ) : (
        <p className="text-[12px] leading-relaxed text-fg-muted">
          Войдите в свой аккаунт на claude.ai в окне, которое откроет панель. Пароль вводится на сайте Anthropic, панель
          его не видит. Затем панель раз в 5 минут запрашивает у claude.ai ваши лимиты, как это делает страница
          настроек. Это неофициальный способ: если Anthropic его изменит, обновление остановится.
        </p>
      )}

      <div className="mt-3 flex flex-wrap justify-end gap-2">
        {!web.enabled && (
          <button className={button} disabled={busy} onClick={() => run("claude_web_login")}>
            <LogIn className="size-3.5" /> Войти в claude.ai
          </button>
        )}
        {web.enabled && web.needsLogin && (
          <button className={button} disabled={busy} onClick={() => run("claude_web_login")}>
            <LogIn className="size-3.5" /> Войти снова
          </button>
        )}
        {web.enabled && !web.needsLogin && (
          <button className={button} disabled={busy || web.fetching} onClick={() => run("claude_web_refresh")}>
            <RefreshCw className={clsx("size-3.5", web.fetching && "animate-spin")} /> Обновить
          </button>
        )}
        {web.enabled && (
          <button className={button} disabled={busy} onClick={() => run("claude_web_logout")}>
            <LogOut className="size-3.5" /> Выйти
          </button>
        )}
        {claudeConnected && (
          <button className={button} disabled={busy} onClick={() => run("claude_disconnect")}>
            <Unlink className="size-3.5" /> Отключить Claude Code
          </button>
        )}
      </div>
      {(error || web.error) && (
        <p className="mt-2 text-[12px] leading-relaxed text-warn">{error ?? web.error}</p>
      )}
    </Card>
  );
}

export function AiTab() {
  const { data, isPending } = useAiLimits();
  if (isPending || !data) return null;

  return (
    <div className="flex flex-col gap-3 pb-2">
      <Card title="Спросить Claude" icon={MessageCircleQuestion}>
        <AskBox />
      </Card>

      <Card title="Сессии" icon={Sparkle} action={<AgentsNotifyToggle />}>
        <AgentsList />
      </Card>

      <Card title="Чаты Bionic" icon={MessagesSquare}>
        <BionicChats />
      </Card>

      <ClaudeCard data={data} />

      <Card>
        <div className="mb-3 flex items-center justify-between">
          <span className="text-[15px] font-semibold">Codex</span>
          <div className="flex items-center gap-2">
            {data.codex?.plan && (
              <span className="rounded-md border border-stroke px-1.5 py-0.5 text-[11px] text-fg-muted uppercase">
                {data.codex.plan}
              </span>
            )}
            <BellToggle provider="codex" on={data.alerts.codex} />
          </div>
        </div>
        {data.codex ? (
          <>
            <LimitBars snapshot={data.codex} large />
            <Resets snapshot={data.codex} />
          </>
        ) : (
          <p className="text-[12px] leading-relaxed text-fg-muted">
            Данных нет. Они появятся после первого запроса в Codex CLI или приложении Codex.
          </p>
        )}
      </Card>

      <p className="px-1 text-[11.5px] leading-relaxed text-fg-subtle">
        Лимиты Codex обновляются, когда вы работаете с Codex. Окно, время сброса которого прошло, считается пустым.
        Колокольчик включает уведомление со звуком, когда израсходованный лимит сбрасывается, даже если панель закрыта.
      </p>
    </div>
  );
}
