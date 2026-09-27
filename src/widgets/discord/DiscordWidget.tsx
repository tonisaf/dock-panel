import { useCallback, useRef, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, HeadphoneOff, Headphones, Mic, MicOff, PhoneOff, SlidersHorizontal, Volume2 } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { Slider } from "../../components/Slider";
import { AnchoredMenu, menuItem } from "../spotify/Menu";
import { usePanelStore } from "../../store";
import { useDiscord, useDiscordActions, type DiscordChannel, type DiscordMember, type DiscordVoice, type VoiceChange } from "../../discord/api";

const SHOWN_AVATARS = 6;

export function Avatar({ m, size = "size-6" }: { m: DiscordMember; size?: string }) {
  const [broken, setBroken] = useState(false);
  return (
    <span
      title={m.name + (m.deafened ? " · звук выключен" : m.muted ? " · микрофон выключен" : "")}
      className={clsx(
        "relative grid shrink-0 place-items-center rounded-full bg-ink/10 text-[10px] font-medium text-fg-muted ring-2 transition-shadow",
        size,
        m.speaking ? "ring-emerald-400" : "ring-transparent",
      )}
    >
      {broken ? (
        m.name.slice(0, 1).toUpperCase()
      ) : (
        <img src={m.avatar} alt="" onError={() => setBroken(true)} className="size-full rounded-full object-cover" />
      )}
      {(m.muted || m.deafened) && (
        <span className="absolute -right-1 -bottom-1 grid size-3.5 place-items-center rounded-full bg-popover text-warn">
          {m.deafened ? <HeadphoneOff className="size-2.5" /> : <MicOff className="size-2.5" />}
        </span>
      )}
    </span>
  );
}

function Members({ members }: { members: DiscordMember[] }) {
  const extra = members.length - SHOWN_AVATARS;
  return (
    <div className="flex items-center gap-1.5">
      {members.slice(0, SHOWN_AVATARS).map((m) => (
        <Avatar key={m.id} m={m} />
      ))}
      {extra > 0 && <span className="text-[11.5px] text-fg-subtle tabular-nums">+{extra}</span>}
    </div>
  );
}

function RoundButton({
  on,
  danger,
  title,
  onClick,
  children,
}: {
  on?: boolean;
  danger?: boolean;
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      aria-pressed={on}
      className={clsx(
        "grid size-8 shrink-0 place-items-center rounded-full transition-colors",
        danger ? "bg-red-500/15 text-red-400 hover:bg-red-500/25" : "bg-ink/8 text-fg-muted hover:bg-ink/14 hover:text-fg",
      )}
    >
      {children}
    </button>
  );
}

/** Microphone volume, voice activity or push-to-talk, and the input device. */
function MicMenu({ voice, onChange }: { voice: DiscordVoice; onChange: (c: VoiceChange) => void }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <>
      <button
        ref={anchor}
        onClick={() => setOpen(!open)}
        title="Настройки микрофона"
        className={clsx(
          "grid size-8 shrink-0 place-items-center rounded-full text-fg-subtle transition-colors hover:bg-ink/10 hover:text-fg",
          open && "bg-ink/10 text-fg",
        )}
      >
        <SlidersHorizontal className="size-4" />
      </button>
      {open && (
        <AnchoredMenu anchor={anchor} onClose={close} className="w-64">
          <div className="px-2.5 pt-1.5 pb-2.5">
            <div className="mb-2 flex justify-between text-[12px] text-fg-muted">
              Громкость микрофона <span className="tabular-nums">{Math.round(voice.inputVolume)}</span>
            </div>
            <Slider label="Громкость микрофона" value={voice.inputVolume} min={0} max={100} onCommit={(v) => onChange({ inputVolume: v })} />
          </div>
          <div className="border-t border-ink/10 pt-1">
            {(
              [
                ["VOICE_ACTIVITY", "Голосовая активность"],
                ["PUSH_TO_TALK", "Режим рации"],
              ] as const
            ).map(([mode, label]) => (
              <button key={mode} className={menuItem} onClick={() => onChange({ mode })}>
                <Check className={clsx("size-3.5", voice.mode !== mode && "invisible")} /> {label}
              </button>
            ))}
          </div>
          {voice.inputDevices.length > 1 && (
            <div className="mt-1 max-h-48 overflow-y-auto border-t border-ink/10 pt-1">
              {voice.inputDevices.map((d) => (
                <button key={d.id} className={menuItem} onClick={() => onChange({ inputDevice: d.id })}>
                  <Check className={clsx("size-3.5 shrink-0", voice.inputDevice !== d.id && "invisible")} />
                  <span className="truncate">{d.name}</span>
                </button>
              ))}
            </div>
          )}
        </AnchoredMenu>
      )}
    </>
  );
}

/** A watched channel with who is in it; a click joins it. */
function ChannelRow({ c, onJoin }: { c: DiscordChannel; onJoin: () => void }) {
  return (
    <button onClick={onJoin} title="Зайти в канал" className="-mx-1.5 flex items-center gap-2.5 rounded-xl px-1.5 py-1.5 text-left hover:bg-surface">
      <Volume2 className={clsx("size-4 shrink-0", c.members.length ? "text-ok" : "text-fg-subtle")} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px]">{c.name}</div>
        <div className="truncate text-[11.5px] text-fg-subtle">{c.guildName}</div>
      </div>
      {c.members.length > 0 ? <Members members={c.members} /> : <span className="text-[11.5px] text-fg-subtle">пусто</span>}
    </button>
  );
}

export function DiscordWidget() {
  const setTab = usePanelStore((s) => s.setTab);
  const { data: s } = useDiscord();
  const actions = useDiscordActions();
  const [error, setError] = useState<string | null>(null);
  const run = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };

  if (!s) return null;
  if (!s.configured) {
    return (
      <Card title="Discord" icon={Headphones}>
        <button onClick={() => setTab("settings")} className="text-left text-[12px] text-fg-subtle hover:text-fg">
          Подключите Discord в настройках →
        </button>
      </Card>
    );
  }
  if (!s.connected || !s.voice) {
    return (
      <Card title="Discord" icon={Headphones}>
        <div className="flex items-center justify-between gap-3">
          <p className={clsx("text-[12px] leading-relaxed", s.error ? "text-warn" : "text-fg-subtle")}>{s.error ?? "Discord не запущен"}</p>
          {!s.error && (
            <button
              onClick={() => openUrl("discord://").catch((e) => setError(String(e)))}
              className="shrink-0 rounded-lg border border-stroke px-2.5 py-1 text-[12px] text-fg-muted hover:bg-ink/8 hover:text-fg"
            >
              Запустить
            </button>
          )}
        </div>
        {error && <p className="mt-1.5 text-[12px] text-warn">{error}</p>}
      </Card>
    );
  }

  const { voice, current } = s;
  const others = s.watched.filter((c) => c.id !== current?.id);
  return (
    <Card title="Discord" icon={Headphones}>
      <div className="flex items-center gap-2">
        <RoundButton on={voice.mute} danger={voice.mute} title={voice.mute ? "Включить микрофон" : "Выключить микрофон"} onClick={() => run(actions.voice({ mute: !voice.mute }))}>
          {voice.mute ? <MicOff className="size-4" /> : <Mic className="size-4" />}
        </RoundButton>
        <RoundButton on={voice.deaf} danger={voice.deaf} title={voice.deaf ? "Включить звук" : "Выключить звук"} onClick={() => run(actions.voice({ deaf: !voice.deaf }))}>
          {voice.deaf ? <HeadphoneOff className="size-4" /> : <Headphones className="size-4" />}
        </RoundButton>
        <div className="min-w-0 flex-1 px-1">
          {current ? (
            <>
              <div className="truncate text-[13px] text-ok">{current.name}</div>
              <div className="truncate text-[11.5px] text-fg-subtle">{current.guildName}</div>
            </>
          ) : (
            <div className="text-[12px] text-fg-subtle">Не в голосовом канале</div>
          )}
        </div>
        <MicMenu voice={voice} onChange={(c) => run(actions.voice(c))} />
        {current && (
          <RoundButton danger title="Отключиться" onClick={() => run(actions.join(null))}>
            <PhoneOff className="size-4" />
          </RoundButton>
        )}
      </div>
      {current && current.members.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {current.members.map((m) => (
            <Avatar key={m.id} m={m} size="size-7" />
          ))}
        </div>
      )}
      {others.length > 0 && (
        <div className="mt-2.5 flex flex-col border-t border-stroke pt-1.5">
          {others.map((c) => (
            <ChannelRow key={c.id} c={c} onJoin={() => run(actions.join(c.id))} />
          ))}
        </div>
      )}
      {s.watched.length === 0 && (
        <button onClick={() => setTab("settings")} className="mt-2.5 text-left text-[12px] text-fg-subtle hover:text-fg">
          Выберите каналы, за которыми следить →
        </button>
      )}
      {error && <p className="mt-1.5 text-[12px] leading-relaxed text-warn">{error}</p>}
    </Card>
  );
}
