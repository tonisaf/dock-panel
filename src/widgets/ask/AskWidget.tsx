import { MessageCircleQuestion } from "lucide-react";
import { Card } from "../../components/Card";
import { AskBox } from "../../ask/AskBox";

export function AskWidget() {
  return (
    <Card title="Спросить Claude" icon={MessageCircleQuestion} className="hover:bg-surface">
      <AskBox compact />
    </Card>
  );
}
