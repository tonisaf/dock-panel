import { Check, Sparkle } from "lucide-react";
import { Card } from "../../components/Card";
import { AgentsList, AgentsNotifyToggle } from "../../agents/AgentsList";

import { WidgetMenu } from "../../components/WidgetMenu";
import { usePrefs } from "../../lib/prefs";

export function AgentsWidget() {
  const limit = usePrefs((s) => s.agentsChatLimit);
  const setLimit = usePrefs((s) => s.setAgentsChatLimit);
  return (
    <Card title="Агенты" icon={Sparkle} action={<div className="flex items-center gap-0.5"><AgentsNotifyToggle /><WidgetMenu items={[3, 5, 10, 20, 0].map((value) => ({
      label: value ? `Последние чаты: ${value}` : "Все доступные чаты",
      icon: value === limit ? Check : undefined,
      onClick: () => setLimit(value),
    }))} /></div>} className="hover:bg-surface">
      <AgentsList compact limit={limit} />
    </Card>
  );
}
