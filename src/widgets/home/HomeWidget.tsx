import { useState, type ReactNode } from "react";
import {
  ChevronDown,
  House,
  Lightbulb,
  LightbulbOff,
  Loader2,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  SkipBack,
  SkipForward,
  Speaker as SpeakerIcon,
  EyeOff,
  Trash2,
  Volume2,
  VolumeX,
} from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { Slider } from "../../components/Slider";
import {
  kelvinToRgb,
  lampColor,
  useHome,
  useHomeActions,
  type Lamp,
  type LampChange,
  type Light,
  type Speaker,
} from "./api";

/** Colours offered for RGB lamps, plus the warm white they usually run at. */
const SWATCHES = [0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x00c7be, 0x007aff, 0xaf52de, 0xff2d55];

const report = (e: unknown) => console.error(e);

function Toggle({ on, onChange, disabled }: { on: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      className={clsx(
        "relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-40",
        on ? "bg-accent" : "bg-ink/15",
      )}
    >
      <span
        className={clsx(
          "absolute top-0.5 size-4 rounded-full bg-white shadow transition-[left]",
          on ? "left-[18px]" : "left-0.5",
        )}
      />
    </button>
  );
}

/** Inline rename; an empty name goes back to the default one. */
function Rename({ id, name, onDone }: { id: string; name: string; onDone: () => void }) {
  const { rename } = useHomeActions();
  const [text, setText] = useState(name);
  const save = () => rename(id, text).catch(report).finally(onDone);
  return (
    <input
      autoFocus
      value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") save();
        if (e.key === "Escape") {
          e.stopPropagation();
          onDone();
        }
      }}
      onBlur={save}
      className="h-7 w-full rounded-md border border-stroke bg-field px-2 text-[13px] outline-none focus:border-accent/50"
    />
  );
}

function RowTools({ id, name, offline, onRename }: { id: string; name: string; offline: boolean; onRename: () => void }) {
  const { forget } = useHomeActions();
  const tool = "flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11.5px] text-fg-subtle hover:bg-ink/10 hover:text-fg";
  return (
    <div className="flex gap-1">
      <button className={tool} onClick={onRename}>
        <Pencil className="size-3" /> Переименовать
      </button>
      {/* Yandex would list its lights again, so for them this hides instead. */}
      {id.startsWith("yandex:") ? (
        <button className={tool} onClick={() => forget(id).catch(report)} title={`Скрыть «${name}» из панели`}>
          <EyeOff className="size-3" /> Скрыть
        </button>
      ) : (
        offline && (
          <button className={tool} onClick={() => forget(id).catch(report)} title={`Забыть «${name}»`}>
            <Trash2 className="size-3" /> Забыть
          </button>
        )
      )}
    </div>
  );
}

/** Brightness, colour temperature and colour swatches for one light of a lamp. */
function LightControls({
  lamp,
  light,
  supportsCt,
  supportsRgb,
  onChange,
}: {
  lamp: Lamp;
  light: Light;
  supportsCt: boolean;
  supportsRgb: boolean;
  onChange: (c: LampChange) => void;
}) {
  const warm = kelvinToRgb(lamp.ctMin).join(" ");
  const cold = kelvinToRgb(lamp.ctMax).join(" ");
  return (
    <>
      <label className="flex flex-col gap-1.5 text-[11.5px] text-fg-subtle">
        Яркость
        <Slider label="Яркость" value={light.bright} min={1} max={100} onCommit={(bright) => onChange({ bright })} />
      </label>
      {supportsCt && (
        <label className="flex flex-col gap-1.5 text-[11.5px] text-fg-subtle">
          Температура · {light.ct} K
          <Slider
            label="Температура"
            value={Math.min(lamp.ctMax, Math.max(lamp.ctMin, light.ct))}
            min={lamp.ctMin}
            max={lamp.ctMax}
            step={100}
            track={`linear-gradient(to right, rgb(${warm}), rgb(${cold}))`}
            onCommit={(ct) => onChange({ ct })}
          />
        </label>
      )}
      {supportsRgb && (
        <div className="flex flex-wrap gap-1.5">
          {SWATCHES.map((rgb) => (
            <button
              key={rgb}
              onClick={() => onChange({ rgb })}
              title="Цвет"
              className={clsx(
                "size-6 rounded-full ring-offset-2 ring-offset-popover transition-transform hover:scale-110",
                light.colorMode === 1 && light.rgb === rgb && "ring-2 ring-fg/60",
              )}
              style={{ background: `#${rgb.toString(16).padStart(6, "0")}` }}
            />
          ))}
        </div>
      )}
    </>
  );
}

function LampRow({ lamp }: { lamp: Lamp }) {
  const { setLamp } = useHomeActions();
  const [open, setOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const s = lamp.state;
  const on = !!s?.power;
  const bg = s?.bg;
  const change = (c: LampChange) => {
    setError(null);
    setLamp(lamp, c).catch((e) => setError(String(e)));
  };
  const status = !s ? "не отвечает" : [on ? `${s.bright}%` : "выключена", bg?.power && "подсветка"].filter(Boolean).join(" · ");

  return (
    <div className={clsx("rounded-xl transition-colors", open && "bg-ink/5")}>
      <div
        role="button"
        onClick={() => s && setOpen(!open)}
        className="flex cursor-default items-center gap-2.5 rounded-xl px-2 py-1.5 hover:bg-ink/5"
      >
        <div
          className="grid size-8 shrink-0 place-items-center rounded-full transition-colors"
          style={on && s ? { background: `color-mix(in srgb, ${lampColor(s)} ${20 + s.bright / 2}%, transparent)` } : undefined}
        >
          {s ? (
            <Lightbulb className="size-4" style={{ color: on ? lampColor(s) : undefined }} fill={on ? "currentColor" : "none"} />
          ) : (
            <LightbulbOff className="size-4 text-fg-subtle" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          {renaming ? (
            <Rename id={lamp.id} name={lamp.name} onDone={() => setRenaming(false)} />
          ) : (
            <div className="truncate text-[13.5px]">{lamp.name}</div>
          )}
          <div className={clsx("text-[11.5px]", error ? "text-warn" : "text-fg-subtle")}>
            {error ?? status}
          </div>
        </div>
        {s && <ChevronDown className={clsx("size-3.5 text-fg-subtle transition-transform", open && "rotate-180")} />}
        <Toggle on={on} disabled={!s} onChange={(power) => change({ power })} />
      </div>

      {open && s && (
        <div className="flex flex-col gap-3 px-3 pt-1 pb-3">
          <LightControls
            lamp={lamp}
            light={s}
            supportsCt={lamp.supportsCt}
            supportsRgb={lamp.supportsRgb}
            onChange={change}
          />
          {bg && (
            <div className="flex flex-col gap-3 border-t border-stroke pt-3">
              <div className="flex items-center gap-2 text-[12px]">
                <span
                  className="size-2.5 rounded-full bg-ink/15"
                  style={bg.power ? { background: lampColor(bg) } : undefined}
                />
                <span className="flex-1">Подсветка</span>
                <Toggle on={bg.power} onChange={(power) => change({ power, background: true })} />
              </div>
              <LightControls
                lamp={lamp}
                light={bg}
                supportsCt={lamp.bgSupportsCt}
                supportsRgb={lamp.bgSupportsRgb}
                onChange={(c) => change({ ...c, background: true })}
              />
            </div>
          )}
          <RowTools id={lamp.id} name={lamp.name} offline={false} onRename={() => setRenaming(true)} />
        </div>
      )}
      {!s && (
        <div className="px-2 pb-1.5 pl-12">
          <RowTools id={lamp.id} name={lamp.name} offline onRename={() => setRenaming(true)} />
        </div>
      )}
    </div>
  );
}

function SpeakerRow({ speaker }: { speaker: Speaker }) {
  const { controlSpeaker } = useHomeActions();
  const [renaming, setRenaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const s = speaker.state;
  const control = (c: Parameters<typeof controlSpeaker>[1]) => {
    setError(null);
    controlSpeaker(speaker, c).catch((e) => setError(String(e)));
  };
  const playing = s?.playerState === "PLAYING" || s?.playerState === "BUFFERING";
  const nowPlaying = s?.title ? [s.title, s.artist].filter(Boolean).join(" — ") : null;
  const btn = "grid size-7 place-items-center rounded-full text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-30";

  return (
    <div className="flex flex-col gap-1.5 rounded-xl px-2 py-1.5">
      <div className="flex items-center gap-2.5">
        <div className="grid size-8 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/8">
          {s?.image ? (
            <img src={s.image} className="size-full object-cover" draggable={false} />
          ) : (
            <SpeakerIcon className={clsx("size-4", s ? "text-fg-muted" : "text-fg-subtle")} />
          )}
        </div>
        <div className="min-w-0 flex-1">
          {renaming ? (
            <Rename id={speaker.id} name={speaker.name} onDone={() => setRenaming(false)} />
          ) : (
            <div className="truncate text-[13.5px]" onDoubleClick={() => setRenaming(true)} title="Двойной клик — переименовать">
              {speaker.name}
            </div>
          )}
          <div className={clsx("truncate text-[11.5px]", error ? "text-warn" : "text-fg-subtle")}>
            {error ?? (!s ? "не отвечает" : nowPlaying ? `${s.app ? `${s.app} · ` : ""}${nowPlaying}` : (s.app ?? "тишина"))}
          </div>
        </div>
        {s?.playerState && (
          <div className="flex items-center">
            {s.canPrev && (
              <button className={btn} onClick={() => control({ action: "prev" })}>
                <SkipBack className="size-3.5" fill="currentColor" />
              </button>
            )}
            <button className={clsx(btn, "size-8 bg-ink/10")} onClick={() => control({ action: playing ? "pause" : "play" })}>
              {playing ? <Pause className="size-4" fill="currentColor" /> : <Play className="ml-0.5 size-4" fill="currentColor" />}
            </button>
            {s.canNext && (
              <button className={btn} onClick={() => control({ action: "next" })}>
                <SkipForward className="size-3.5" fill="currentColor" />
              </button>
            )}
          </div>
        )}
      </div>
      {s ? (
        <div className="flex items-center gap-2 pl-[42px]">
          <button
            className={btn}
            title={s.muted ? "Включить звук" : "Выключить звук"}
            onClick={() => control({ action: "mute", muted: !s.muted })}
          >
            {s.muted ? <VolumeX className="size-4 text-warn" /> : <Volume2 className="size-4" />}
          </button>
          <Slider
            label="Громкость"
            value={Math.round(s.volume * 100)}
            min={0}
            max={100}
            onCommit={(v) => control({ action: "volume", level: v / 100 })}
          />
          <span className="w-8 text-right text-[11px] text-fg-subtle tabular-nums">{Math.round(s.volume * 100)}</span>
        </div>
      ) : (
        <div className="pl-[42px]">
          <RowTools id={speaker.id} name={speaker.name} offline onRename={() => setRenaming(true)} />
        </div>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex flex-col">
      <div className="px-2 pb-0.5 text-[11px] font-medium text-fg-subtle">{title}</div>
      {children}
    </div>
  );
}

export function HomeWidget() {
  const { data, isPending, isError, error } = useHome();
  const { rescan } = useHomeActions();
  const [scanning, setScanning] = useState(false);
  const search = () => {
    setScanning(true);
    rescan()
      .catch(report)
      .finally(() => setScanning(false));
  };

  const action = (
    <button
      onClick={search}
      disabled={scanning}
      title="Искать устройства в сети"
      className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
    >
      <RefreshCw className={clsx("size-3.5", scanning && "animate-spin")} />
    </button>
  );

  const empty = data && data.lamps.length === 0 && data.speakers.length === 0;
  return (
    <Card title="Дом" icon={House} action={action} className="hover:bg-surface">
      {isPending ? (
        <p className="flex items-center gap-2 text-[12px] text-fg-subtle">
          <Loader2 className="size-3.5 animate-spin" /> Ищу лампы и колонки в сети…
        </p>
      ) : isError ? (
        <p className="text-[12px] text-warn">{String(error)}</p>
      ) : empty ? (
        <div className="flex flex-col gap-1.5 text-[12px] leading-relaxed text-fg-subtle">
          <p>Ничего не найдено в домашней сети.</p>
          <p>
            Лампы Yeelight: в приложении Yeelight откройте лампу → ⚙ → «Управление по локальной сети» (LAN Control) и
            включите. Колонки с Google Cast находятся сами.
          </p>
          <p>Компьютер должен быть в той же Wi-Fi-сети. Если Windows спросит про доступ к сети — разрешите.</p>
          <button onClick={search} className="self-start text-accent hover:underline">
            Искать снова
          </button>
        </div>
      ) : (
        <div className="-mx-2 flex flex-col gap-2">
          {data!.lamps.length > 0 && (
            <Section title="Свет">
              {data!.lamps.map((l) => (
                <LampRow key={l.id} lamp={l} />
              ))}
            </Section>
          )}
          {data!.speakers.length > 0 && (
            <Section title="Колонки">
              {data!.speakers.map((sp) => (
                <SpeakerRow key={sp.id} speaker={sp} />
              ))}
            </Section>
          )}
        </div>
      )}
    </Card>
  );
}
