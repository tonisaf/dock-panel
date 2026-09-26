import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronDown,
  Contrast,
  Loader2,
  Monitor as MonitorIcon,
  MonitorCog,
  MoonStar,
  Power,
  Sun,
  Volume2,
} from "lucide-react";
import clsx from "clsx";
import { Card } from "../../components/Card";
import { usePrefs } from "../../lib/prefs";
import { Slider } from "../../components/Slider";
import { inputName, percent, useMonitors, useSetMonitor, type Feature, type Level, type Monitor } from "./api";

function LevelRow({
  icon: Icon,
  label,
  level,
  onCommit,
}: {
  icon: typeof Sun;
  label: string;
  level: Level;
  onCommit: (v: number) => void;
}) {
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? level.value;
  return (
    <div className="flex items-center gap-2.5" title={label}>
      <Icon className="size-3.5 shrink-0 text-fg-subtle" />
      <Slider label={label} value={level.value} min={0} max={level.max} onCommit={onCommit} onDraft={setDraft} />
      <span className="w-8 shrink-0 text-right text-[11px] text-fg-subtle tabular-nums">
        {Math.round((shown / level.max) * 100)}
      </span>
    </div>
  );
}

function MonitorRow({ monitor }: { monitor: Monitor }) {
  const set = useSetMonitor();
  const [open, setOpen] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const change = (feature: Feature, value: number) => {
    setError(null);
    set(monitor, feature, value).catch((e) => setError(String(e)));
  };
  const { brightness, contrast, volume, input, power } = monitor;
  const hasMore = !!(contrast || volume || input || power);
  // Standby (4) is what the monitor's power button usually does; some only know "off" (5).
  const offCode = power?.options.includes(4) ? 4 : power?.options.includes(5) ? 5 : null;
  const inputs = input ? [...input.options].sort((a, b) => inputName(a).localeCompare(inputName(b))) : [];

  return (
    <div className={clsx("rounded-xl transition-colors", open && "bg-ink/5")}>
      <div
        role="button"
        onClick={() => hasMore && setOpen(!open)}
        className="flex cursor-default items-center gap-2.5 rounded-xl px-2 pt-1.5 hover:bg-ink/5"
      >
        <MonitorIcon className="size-4 shrink-0 text-fg-muted" />
        <div className="min-w-0 flex-1 truncate text-[13.5px]">
          {monitor.name}
          {monitor.primary && <span className="ml-1.5 text-[11px] text-fg-subtle">основной</span>}
        </div>
        {hasMore && <ChevronDown className={clsx("size-3.5 text-fg-subtle transition-transform", open && "rotate-180")} />}
      </div>
      <div className="flex flex-col gap-2.5 px-2 pt-1.5 pb-2 pl-[34px]">
        {brightness ? (
          <LevelRow icon={Sun} label="Яркость" level={brightness} onCommit={(v) => change("brightness", v)} />
        ) : (
          <p className="text-[11.5px] text-fg-subtle">Монитор не отдаёт яркость по DDC/CI</p>
        )}
        {error && <p className="text-[11.5px] text-warn">{error}</p>}

        {open && (
          <>
            {contrast && <LevelRow icon={Contrast} label="Контраст" level={contrast} onCommit={(v) => change("contrast", v)} />}
            {volume && <LevelRow icon={Volume2} label="Громкость" level={volume} onCommit={(v) => change("volume", v)} />}
            {inputs.length > 1 && (
              <div className="flex flex-wrap gap-1">
                {inputs.map((code) => (
                  <button
                    key={code}
                    onClick={() => change("input", code)}
                    className={clsx(
                      "rounded-md px-2 py-0.5 text-[11.5px] transition-colors",
                      input?.current === code ? "bg-accent/20 text-fg" : "bg-ink/8 text-fg-muted hover:bg-ink/12 hover:text-fg",
                    )}
                  >
                    {inputName(code)}
                  </button>
                ))}
              </div>
            )}
            {offCode != null &&
              (confirmOff ? (
                <div className="flex flex-col gap-1.5 text-[11.5px] text-fg-subtle">
                  <p>Включить обратно получится только кнопкой на мониторе.</p>
                  <div className="flex gap-1.5">
                    <button
                      onClick={() => {
                        setConfirmOff(false);
                        change("power", offCode);
                      }}
                      className="rounded-md bg-warn/20 px-2 py-0.5 text-fg hover:bg-warn/30"
                    >
                      Выключить
                    </button>
                    <button onClick={() => setConfirmOff(false)} className="rounded-md px-2 py-0.5 hover:bg-ink/10 hover:text-fg">
                      Отмена
                    </button>
                  </div>
                </div>
              ) : (
                <button
                  onClick={() => setConfirmOff(true)}
                  className="flex items-center gap-1 self-start rounded-md px-1.5 py-0.5 text-[11.5px] text-fg-subtle hover:bg-ink/10 hover:text-fg"
                >
                  <Power className="size-3" /> Выключить монитор
                </button>
              ))}
          </>
        )}
      </div>
    </div>
  );
}

/** One slider for every monitor's brightness, starting from their average. */
function AllBrightness({ monitors }: { monitors: Monitor[] }) {
  const set = useSetMonitor();
  const withBrightness = monitors.filter((m) => m.brightness);
  if (withBrightness.length < 2) return null;
  const average = Math.round(withBrightness.reduce((sum, m) => sum + percent(m.brightness!), 0) / withBrightness.length);
  const commit = (p: number) => {
    for (const m of withBrightness) {
      set(m, "brightness", Math.round((p / 100) * m.brightness!.max)).catch((e) => console.error(e));
    }
  };
  return (
    <div className="px-2 pb-1">
      <div className="pb-1.5 text-[11px] font-medium text-fg-subtle">Все мониторы</div>
      <LevelRow icon={Sun} label="Яркость всех мониторов" level={{ value: average, max: 100 }} onCommit={commit} />
    </div>
  );
}

export function MonitorsWidget() {
  const { data, isPending, isError, error } = useMonitors();
  // Folded by default: it's reached for rarely and takes a lot of room.
  const collapsed = usePrefs((s) => s.collapsedWidgets.monitors ?? true);
  const setCollapsed = usePrefs((s) => s.setWidgetCollapsed);
  const levels = data?.flatMap((m) => (m.brightness ? [percent(m.brightness)] : [])) ?? [];
  const summary = levels.length
    ? `${levels.length} · ${Math.round(levels.reduce((a, b) => a + b, 0) / levels.length)}%`
    : undefined;

  const blackout = (
    <button
      onClick={() => invoke("monitors_blackout").catch(console.error)}
      title="Затемнить все мониторы: чёрный экран и минимальная яркость. Клик или любая клавиша — вернуть"
      className="grid size-6 place-items-center rounded-md text-fg-subtle hover:bg-ink/10 hover:text-fg"
    >
      <MoonStar className="size-3.5" />
    </button>
  );

  return (
    <Card
      title="Мониторы"
      icon={MonitorCog}
      className="hover:bg-surface"
      action={blackout}
      summary={summary}
      collapsed={collapsed}
      onToggle={() => setCollapsed("monitors", !collapsed)}
    >
      {isPending ? (
        <p className="flex items-center gap-2 text-[12px] text-fg-subtle">
          <Loader2 className="size-3.5 animate-spin" /> Спрашиваю мониторы…
        </p>
      ) : isError ? (
        <p className="text-[12px] text-warn">{String(error)}</p>
      ) : data.length === 0 ? (
        <p className="text-[12px] leading-relaxed text-fg-subtle">
          Ни один монитор не ответил. Включите DDC/CI в меню монитора; встроенные экраны ноутбуков тут не управляются.
        </p>
      ) : (
        <div className="-mx-2 flex flex-col gap-1">
          <AllBrightness monitors={data} />
          {data.map((m) => (
            <MonitorRow key={m.id} monitor={m} />
          ))}
        </div>
      )}
    </Card>
  );
}
