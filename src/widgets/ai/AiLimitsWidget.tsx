import { Sparkles } from "lucide-react";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { useAiLimits } from "./api";
import { LimitBars } from "./LimitBars";

export function AiLimitsWidget() {
  const { data } = useAiLimits();
  const setTab = usePanelStore((s) => s.setTab);
  const providers = [
    { name: "Claude", snapshot: data?.claude },
    { name: "Codex", snapshot: data?.codex },
  ].filter((p) => p.snapshot);

  return (
    <Card title="Лимиты AI" icon={Sparkles}>
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
