import { Mail } from "lucide-react";
import { Card } from "../../components/Card";
import { usePanelStore } from "../../store";
import { shortDate, useMailList, useMailSettings, useUnread } from "../../mail/api";

const SHOWN = 3;

/** Unread count and the newest unread letters; a click opens the mail tab. */
export function MailWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const { data: settings } = useMailSettings();
  const connected = (settings?.accounts.length ?? 0) > 0;
  const { messages } = useMailList(null, true, connected);
  const unread = useUnread().data?.total ?? 0;
  const latest = messages.filter((m) => m.unread).slice(0, SHOWN);

  if (!connected) {
    return (
      <Card title="Почта" icon={Mail}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Подключите Яндекс или Gmail в настройках →
        </button>
      </Card>
    );
  }

  return (
    <Card title="Почта" icon={Mail}>
      <button onClick={() => setTab("mail")} className="flex w-full flex-col gap-2 text-left">
        <div className="flex items-baseline gap-2">
          <span className="font-display text-[28px] leading-none font-semibold tabular-nums">{unread}</span>
          <span className="text-[12.5px] text-fg-muted">{unread === 0 ? "всё прочитано" : "непрочитанных"}</span>
        </div>
        {latest.length > 0 && (
          <div className="flex flex-col gap-1.5 border-t border-stroke pt-2">
            {latest.map((m) => (
              <div key={`${m.account}/${m.uid}`} className="min-w-0">
                <div className="flex items-baseline gap-2 text-[12.5px]">
                  <span className="min-w-0 flex-1 truncate font-medium">{m.fromName || m.fromEmail}</span>
                  <span className="shrink-0 text-[11px] text-fg-subtle tabular-nums">{shortDate(m.date)}</span>
                </div>
                <div className="truncate text-[12px] text-fg-subtle">{m.subject}</div>
              </div>
            ))}
          </div>
        )}
      </button>
    </Card>
  );
}
