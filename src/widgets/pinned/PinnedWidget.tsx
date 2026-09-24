import { Pin } from "lucide-react";
import { Card } from "../../components/Card";
import { AppGrid } from "../../components/AppTile";
import { PinFromDisk } from "../../components/PinFromDisk";
import { usePinnedEntries } from "../../lib/apps";

const HOME_PINNED_MAX = 8;

export function PinnedWidget() {
  const pinned = usePinnedEntries().slice(0, HOME_PINNED_MAX);

  return (
    <Card title="Закреплённые" icon={Pin} className="hover:bg-surface" action={<PinFromDisk />}>
      {pinned.length > 0 ? (
        <AppGrid apps={pinned} />
      ) : (
        <p className="text-[12px] leading-relaxed text-fg-subtle">
          Закрепите приложение правым кликом во вкладке «Приложения», а файл или папку — кнопками выше или
          перетащив их на панель.
        </p>
      )}
    </Card>
  );
}
