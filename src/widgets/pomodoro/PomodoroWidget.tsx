import { useCallback, useRef, useState } from "react";
import { Minus, Pause, Play, Plus, RotateCcw, Settings2, SkipForward, Timer } from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { Toggle } from "../../components/Toggle";
import { AnchoredMenu } from "../spotify/Menu";
import { clock, usePomodoro, usePomodoroActions, useTimeLeft, type Phase, type PomodoroSettings } from "./api";

const PHASES: { id: Phase; label: string }[] = [
  { id: "focus", label: "Фокус" },
  { id: "short", label: "Перерыв" },
  { id: "long", label: "Длинный" },
];

const RING = 52;
const CIRCUMFERENCE = 2 * Math.PI * RING;

function Stepper({ label, value, min, max, unit, onChange }: { label: string; value: number; min: number; max: number; unit?: string; onChange: (v: number) => void }) {
  const set = (v: number) => onChange(Math.min(max, Math.max(min, v)));
  const button = "grid size-6 place-items-center rounded-md text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-30";
  return (
    <div className="flex items-center justify-between gap-3 px-2.5 py-1.5 text-[13px]">
      {label}
      <div className="flex items-center gap-1">
        <button className={button} disabled={value <= min} onClick={() => set(value - 1)} aria-label="Меньше">
          <Minus className="size-3.5" />
        </button>
        <span className="w-12 text-center tabular-nums">
          {value}
          {unit && <span className="text-fg-subtle"> {unit}</span>}
        </span>
        <button className={button} disabled={value >= max} onClick={() => set(value + 1)} aria-label="Больше">
          <Plus className="size-3.5" />
        </button>
      </div>
    </div>
  );
}

function SettingsMenu({ settings, onChange }: { settings: PomodoroSettings; onChange: (s: PomodoroSettings) => void }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const patch = (p: Partial<PomodoroSettings>) => onChange({ ...settings, ...p });
  return (
    <>
      <button
        ref={anchor}
        onClick={() => setOpen(!open)}
        title="Настройки помодоро"
        className={clsx("grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg", open && "bg-ink/10 text-fg")}
      >
        <Settings2 className="size-3.5" />
      </button>
      {open && (
        <AnchoredMenu anchor={anchor} onClose={close} className="w-64">
          <Stepper label="Фокус" unit="мин" value={settings.focusMin} min={1} max={180} onChange={(v) => patch({ focusMin: v })} />
          <Stepper label="Перерыв" unit="мин" value={settings.shortMin} min={1} max={60} onChange={(v) => patch({ shortMin: v })} />
          <Stepper label="Длинный перерыв" unit="мин" value={settings.longMin} min={1} max={90} onChange={(v) => patch({ longMin: v })} />
          <Stepper label="Длинный после" value={settings.longEvery} min={1} max={12} onChange={(v) => patch({ longEvery: v })} />
          <div className="mt-1 flex items-center justify-between gap-3 border-t border-ink/10 px-2.5 pt-2 pb-1.5 text-[13px]">
            Следующая фаза сама
            <Toggle on={settings.autoStart} onChange={(v) => patch({ autoStart: v })} />
          </div>
        </AnchoredMenu>
      )}
    </>
  );
}

export function PomodoroWidget() {
  const { data: s } = usePomodoro();
  const actions = usePomodoroActions();
  const left = useTimeLeft(s);
  const [error, setError] = useState<string | null>(null);
  const run = (p: Promise<unknown>) => {
    setError(null);
    p.catch((e) => setError(String(e)));
  };
  if (!s) return null;

  const focus = s.phase === "focus";
  const progress = s.totalMs ? 1 - left / s.totalMs : 0;
  const started = s.running || left < s.totalMs;
  const tone = focus ? "text-rose-500" : "text-emerald-500";
  const every = Math.max(1, s.settings.longEvery);

  return (
    <Card
      title="Помодоро"
      icon={Timer}
      action={<SettingsMenu settings={s.settings} onChange={(v) => run(actions.setSettings(v))} />}
    >
      <div className="flex rounded-lg border border-stroke p-0.5">
        {PHASES.map((p) => (
          <button
            key={p.id}
            onClick={() => p.id !== s.phase && run(actions.setPhase(p.id))}
            className={clsx(
              "flex-1 rounded-md py-1 text-[12px] transition-colors",
              s.phase === p.id ? "bg-ink/10 text-fg" : "text-fg-muted hover:text-fg",
            )}
          >
            {p.label}
          </button>
        ))}
      </div>

      <div className="mt-3 flex items-center justify-center gap-5">
        <button onClick={() => run(actions.reset())} disabled={!started} title="Сначала" className="grid size-9 place-items-center rounded-full text-fg-muted hover:bg-ink/10 hover:text-fg disabled:opacity-30">
          <RotateCcw className="size-4" />
        </button>

        <button
          onClick={() => run(s.running ? actions.pause() : actions.start())}
          title={s.running ? "Пауза" : started ? "Продолжить" : "Старт"}
          className="group relative grid size-32 place-items-center"
        >
          <svg viewBox="0 0 120 120" className={clsx("absolute inset-0 -rotate-90", tone)}>
            <circle cx="60" cy="60" r={RING} fill="none" stroke="currentColor" strokeOpacity={0.15} strokeWidth="6" />
            <circle
              cx="60"
              cy="60"
              r={RING}
              fill="none"
              stroke="currentColor"
              strokeWidth="6"
              strokeLinecap="round"
              strokeDasharray={CIRCUMFERENCE}
              strokeDashoffset={CIRCUMFERENCE * (1 - progress)}
              className="transition-[stroke-dashoffset] duration-300 ease-linear"
            />
          </svg>
          <div className="flex flex-col items-center">
            <span className={clsx("font-display text-[28px] leading-none font-semibold tabular-nums", !s.running && started && "opacity-60")}>
              {clock(left)}
            </span>
            <span className="mt-1.5 flex items-center gap-1 text-[11.5px] text-fg-subtle group-hover:text-fg">
              {s.running ? <Pause className="size-3" /> : <Play className="size-3" />}
              {s.running ? "пауза" : started ? "продолжить" : "старт"}
            </span>
          </div>
        </button>

        <button onClick={() => run(actions.skip())} title="Пропустить фазу" className="grid size-9 place-items-center rounded-full text-fg-muted hover:bg-ink/10 hover:text-fg">
          <SkipForward className="size-4" />
        </button>
      </div>

      <div className="mt-3 flex items-center justify-between text-[12px] text-fg-subtle">
        <div className="flex items-center gap-1" title={`До длинного перерыва: ${Math.max(0, every - s.doneInCycle)}`}>
          {Array.from({ length: every }, (_, i) => (
            <span key={i} className={clsx("size-2 rounded-full", i < s.doneInCycle ? "bg-rose-500" : "bg-ink/15")} />
          ))}
        </div>
        <span>
          Сегодня: <span className="text-fg tabular-nums">{s.today}</span>
        </span>
      </div>
      {error && <p className="mt-1.5 text-[12px] text-warn">{error}</p>}
    </Card>
  );
}
