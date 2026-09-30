import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { RefreshCw, Sparkles } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { useAiLimits } from "./api";
import { LimitBars } from "./LimitBars";

/** Rereads the limits now, without waiting for the periodic refresh. */
function RefreshButton({ fetching }: { fetching: boolean }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const spinning = busy || fetching;
  const refresh = async () => {
    setBusy(true);
    try {
      await invoke("ai_limits_refresh");
      await queryClient.invalidateQueries({ queryKey: ["ai-limits"] });
    } catch (e) {
      console.error(e);
    } finally {
      // Long enough to see that something happened.
      setTimeout(() => setBusy(false), 600);
    }
  };
  return (
    <button
      onClick={refresh}
      disabled={spinning}
      title="Обновить сейчас"
      className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg disabled:hover:bg-transparent"
    >
      <RefreshCw className={clsx("size-3.5", spinning && "animate-spin")} />
    </button>
  );
}

export function AiLimitsWidget() {
  const { data } = useAiLimits();
  const setTab = usePanelStore((s) => s.setTab);
  const providers = [
    { name: "Claude", snapshot: data?.claude },
    { name: "Codex", snapshot: data?.codex },
  ].filter((p) => p.snapshot);

  return (
    <Card title="Лимиты AI" icon={Sparkles} action={<RefreshButton fetching={!!data?.claudeWeb.fetching} />}>
      {providers.length === 0 ? (
        <button onClick={() => setTab("ai")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Нет данных. Откройте вкладку AI, чтобы войти в claude.ai →
        </button>
      ) : (
        <div className="flex flex-col gap-3">
          {providers.map(({ name, snapshot }) => (
            <div key={name} className="flex flex-col gap-1.5">
              <span className="text-[12px] font-semibold">{name}</span>
              <LimitBars snapshot={snapshot!} />
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}
