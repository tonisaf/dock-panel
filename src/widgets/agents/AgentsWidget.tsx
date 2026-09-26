import { Sparkle } from "lucide-react";
import { Card } from "../../components/Card";
import { AgentsList, AgentsNotifyToggle } from "../../agents/AgentsList";

export function AgentsWidget() {
  return (
    <Card title="Агенты" icon={Sparkle} action={<AgentsNotifyToggle />} className="hover:bg-surface">
      <AgentsList compact />
    </Card>
  );
}
