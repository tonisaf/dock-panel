import { useEffect, useState, type ReactNode } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { motion } from "motion/react";
import clsx from "clsx";
import { CityPicker } from "../components/CityPicker";
import { NotionSettings } from "../components/NotionSettings";
import { CalendarSettings } from "../components/CalendarSettings";
import { SpotifySettings } from "../components/SpotifySettings";
import { UpdateSettings } from "../components/UpdateSettings";
import { MAX_WIDTH, MIN_WIDTH, WIDTH_PRESETS, usePanelSettings, usePanelWidth, type Edge } from "../lib/panelWidth";
import { usePrefs, type ThemeMode } from "../lib/prefs";

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 px-3.5 py-3">
      <div>
        <div className="text-[14px]">{label}</div>
        {hint && <div className="mt-0.5 text-[12px] text-fg-subtle">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={clsx(
        "flex h-6 w-11 shrink-0 items-center rounded-full border px-0.5 transition-colors",
        on ? "justify-end border-accent bg-accent" : "justify-start border-ink/25 bg-transparent",
      )}
    >
      <motion.span layout className={clsx("size-4 rounded-full", on ? "bg-on-accent" : "bg-ink/70")} />
    </button>
  );
}

function WidthRow() {
  const { width, setWidth } = usePanelWidth();
  const [draft, setDraft] = useState<number | null>(null);
  const value = draft ?? width;

  return (
    <div className="flex flex-col gap-2.5 px-3.5 py-3">
      <div className="flex items-center justify-between">
        <div>
          <div className="text-[14px]">Ширина панели</div>
          <div className="mt-0.5 text-[12px] text-fg-subtle">Шире — больше колонок виджетов. Можно тянуть за край панели</div>
        </div>
        <span className="text-[12px] text-fg-muted tabular-nums">{value} px</span>
      </div>
      <input
        type="range"
        min={MIN_WIDTH}
        max={MAX_WIDTH}
        step={10}
        value={value}
        onChange={(e) => {
          const next = Number(e.target.value);
          setDraft(next);
          setWidth(next, false);
        }}
        onPointerUp={() => {
          if (draft != null) setWidth(draft, true).then(() => setDraft(null));
        }}
        onKeyUp={() => {
          if (draft != null) setWidth(draft, true).then(() => setDraft(null));
        }}
        className="w-full accent-[var(--color-accent)]"
      />
      <div className="flex gap-1.5">
        {WIDTH_PRESETS.map((p) => (
          <button
            key={p.width}
            onClick={() => setWidth(p.width, true)}
            className={clsx(
              "flex-1 rounded-lg border py-1 text-[12px] transition-colors",
              width === p.width
                ? "border-accent/50 bg-accent/15 text-fg"
                : "border-stroke text-fg-muted hover:bg-ink/8 hover:text-fg",
            )}
          >
            {p.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="flex shrink-0 rounded-lg border border-stroke p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={clsx(
            "rounded-md px-2.5 py-1 text-[12px] transition-colors",
            value === o.value ? "bg-ink/10 text-fg" : "text-fg-muted hover:text-fg",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight"]);

/** KeyboardEvent -> accelerator ("Ctrl+Alt+A"), or null while only modifiers are held. */
function toAccelerator(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  // Bare keys would swallow normal typing; function keys are the exception.
  if (mods.length === 0 && !/^F\d{1,2}$/.test(key)) return null;
  return [...mods, key].join("+");
}

const pretty = (accel: string) => accel.replace(/Super/g, "Win").replace(/\+/g, " + ");

function ShortcutRow() {
  const { shortcut, setShortcut, suspendShortcut } = usePanelSettings();
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    suspendShortcut(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setRecording(false);
      const accel = toAccelerator(e);
      if (!accel) return;
      setRecording(false);
      setShortcut(accel).catch((err) => setError(String(err)));
    };
    // Capture phase: beat the panel's own Esc / Ctrl+1..5 handling.
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      suspendShortcut(false);
    };
  }, [recording]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="px-3.5 py-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="text-[14px]">Горячая клавиша</div>
          <div className="mt-0.5 text-[12px] text-fg-subtle">
            {recording ? "Нажмите сочетание, Esc — отмена" : "Открывает и закрывает панель"}
          </div>
        </div>
        <button
          onClick={() => {
            setError(null);
            setRecording(!recording);
          }}
          className={clsx(
            "shrink-0 rounded-lg border px-2.5 py-1 font-mono text-[12px] transition-colors",
            recording ? "animate-pulse border-accent bg-accent/15 text-fg" : "border-stroke bg-surface text-fg-muted hover:text-fg",
          )}
        >
          {recording ? "…" : pretty(shortcut)}
        </button>
      </div>
      {error && <p className="mt-1.5 text-[12px] text-warn">{error}</p>}
    </div>
  );
}

function EdgeRow() {
  const { edge, setEdge } = usePanelSettings();
  return (
    <Row label="Край экрана">
      <Segmented<Edge>
        value={edge}
        onChange={setEdge}
        options={[
          { value: "left", label: "Слева" },
          { value: "right", label: "Справа" },
        ]}
      />
    </Row>
  );
}

function TaskbarButtonRow() {
  const { taskbarButton, setTaskbarButton } = usePanelSettings();
  return (
    <Row label="Кнопка на панели задач" hint="Слева на панели задач, когда значки по центру">
      <Toggle on={taskbarButton} onChange={setTaskbarButton} />
    </Row>
  );
}

function AppearanceSection() {
  const { theme, setTheme, systemAccent, setSystemAccent } = usePrefs();
  return (
    <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
      <Row label="Тема">
        <Segmented<ThemeMode>
          value={theme}
          onChange={setTheme}
          options={[
            { value: "system", label: "Как в Windows" },
            { value: "dark", label: "Тёмная" },
            { value: "light", label: "Светлая" },
          ]}
        />
      </Row>
      <Row label="Акцентный цвет Windows" hint="Иначе — голубой по умолчанию">
        <Toggle on={systemAccent} onChange={setSystemAccent} />
      </Row>
    </div>
  );
}

export function SettingsTab() {
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    isEnabled().then(setAutostart).catch(() => {});
  }, []);

  const toggleAutostart = async (v: boolean) => {
    await (v ? enable() : disable());
    setAutostart(await isEnabled());
  };

  return (
    <div className="flex flex-col gap-3 pb-2">
      <h3 className="px-1 text-[12px] font-medium text-fg-subtle">Панель</h3>
      <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
        <Row label="Запускать вместе с Windows">
          <Toggle on={autostart} onChange={toggleAutostart} />
        </Row>
        <ShortcutRow />
        <EdgeRow />
        <TaskbarButtonRow />
        <WidthRow />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Оформление</h3>
      <AppearanceSection />
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Notion</h3>
      <div className="rounded-2xl border border-stroke bg-surface">
        <NotionSettings />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Календарь</h3>
      <div className="rounded-2xl border border-stroke bg-surface">
        <CalendarSettings />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Spotify</h3>
      <div className="rounded-2xl border border-stroke bg-surface">
        <SpotifySettings />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Погода</h3>
      <div className="rounded-2xl border border-stroke bg-surface">
        <CityPicker />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Обновления</h3>
      <div className="rounded-2xl border border-stroke bg-surface">
        <UpdateSettings />
      </div>
    </div>
  );
}
