import { useEffect, useState, type ReactNode } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import clsx from "clsx";
import { CityPicker } from "../components/CityPicker";
import { NotionSettings } from "../components/NotionSettings";
import { CalendarSettings } from "../components/CalendarSettings";
import { SpotifySettings } from "../components/SpotifySettings";
import { UpdateSettings } from "../components/UpdateSettings";
import { MAX_WIDTH, MIN_WIDTH, WIDTH_PRESETS, usePanelSettings, usePanelWidth, type Edge } from "../lib/panelWidth";
import { usePrefs, type ThemeMode } from "../lib/prefs";
import { Toggle } from "../components/Toggle";
import { Collapse } from "../components/Collapse";
import { prettyAccelerator, toAccelerator } from "../lib/accelerator";
import { MailSettings } from "../components/MailSettings";
import { GcalSettings } from "../components/GcalSettings";
import { YoutubeSettings } from "../components/YoutubeSettings";
import { YandexSettings } from "../components/YandexSettings";
import { DiscordSettings } from "../components/DiscordSettings";
import {
  CalendarDays,
  ChevronDown,
  CloudSun,
  Download,
  Headphones,
  House,
  Link2,
  Mail,
  Music,
  NotebookPen,
  TvMinimalPlay,
  type LucideIcon,
} from "lucide-react";
import { useMailSettings } from "../mail/api";
import { useGcalStatus } from "../gcal/api";
import { useCalendars as useIcalCalendars } from "../widgets/calendar/api";
import { useNotionStatus } from "../widgets/tasks/api";
import { useSpotifyStatus } from "../widgets/spotify/api";
import { useYoutubeSettings } from "../widgets/youtube/api";
import { useYandexStatus } from "../widgets/home/api";
import { useDiscord } from "../discord/api";
import { useUpdateStatus } from "../lib/updates";

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
          {recording ? "…" : prettyAccelerator(shortcut)}
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
  const {
    taskbarButton,
    setTaskbarButton,
    taskbarPlayer,
    setTaskbarPlayer,
    taskbarMail,
    setTaskbarMail,
    taskbarTasks,
    setTaskbarTasks,
    taskbarAgents,
    setTaskbarAgents,
    taskbarMic,
    setTaskbarMic,
    taskbarPomodoro,
    setTaskbarPomodoro,
  } = usePanelSettings();
  return (
    <>
      <Row label="Кнопка на панели задач" hint="Слева на панели задач, когда значки по центру">
        <Toggle on={taskbarButton} onChange={setTaskbarButton} />
      </Row>
      {taskbarButton && (
        <Row label="Плеер на кнопке" hint="Трек и управление, пока что-то играет">
          <Toggle on={taskbarPlayer} onChange={setTaskbarPlayer} />
        </Row>
      )}
      {taskbarButton && (
        <Row label="Почта на кнопке" hint="Конверт со счётчиком, пока есть непрочитанные письма">
          <Toggle on={taskbarMail} onChange={setTaskbarMail} />
        </Row>
      )}
      {taskbarButton && (
        <Row label="Задачи на кнопке" hint="Счётчик невыполненных задач Google на сегодня и просроченных">
          <Toggle on={taskbarTasks} onChange={setTaskbarTasks} />
        </Row>
      )}
      {taskbarButton && (
        <Row label="Агенты на кнопке" hint="Сколько сессий Claude и Codex закончили работу и ждут вас">
          <Toggle on={taskbarAgents} onChange={setTaskbarAgents} />
        </Row>
      )}
      {taskbarButton && (
        <Row label="Микрофон Discord на кнопке" hint="Пока вы в голосовом канале; клик включает и выключает микрофон">
          <Toggle on={taskbarMic} onChange={setTaskbarMic} />
        </Row>
      )}
      {taskbarButton && (
        <Row label="Помодоро на кнопке" hint="Оставшееся время, пока идёт фокус или перерыв">
          <Toggle on={taskbarPomodoro} onChange={setTaskbarPomodoro} />
        </Row>
      )}
    </>
  );
}

/** "1 ящик", "3 ящика", "5 ящиков". */
function plural(n: number, [one, few, many]: [string, string, string]) {
  const tens = n % 100;
  const word = tens >= 11 && tens <= 14 ? many : n % 10 === 1 ? one : n % 10 >= 2 && n % 10 <= 4 ? few : many;
  return `${n} ${word}`;
}

type Tone = "ok" | "off" | "warn" | "accent";

/**
 * An integration's settings behind a one-line header with its status.
 * Folded by default; the open ones are remembered.
 */
function Group({
  id,
  icon: Icon,
  title,
  status,
  tone = "off",
  children,
}: {
  id: string;
  icon: LucideIcon;
  title: string;
  status?: string;
  tone?: Tone;
  children: ReactNode;
}) {
  const key = `settings.${id}`;
  const collapsed = usePrefs((s) => s.collapsedWidgets[key] ?? true);
  const setCollapsed = usePrefs((s) => s.setWidgetCollapsed);
  return (
    <div className="rounded-2xl border border-stroke bg-surface">
      <button
        onClick={() => setCollapsed(key, !collapsed)}
        aria-expanded={!collapsed}
        className="flex w-full items-center gap-2.5 rounded-2xl px-3.5 py-3 text-left hover:bg-ink/5"
      >
        <Icon className="size-4 shrink-0 text-fg-muted" />
        <span className="text-[14px]">{title}</span>
        {status && (
          <span
            className={clsx(
              "ml-auto flex min-w-0 items-center gap-1.5 truncate text-[12px]",
              tone === "warn" ? "text-warn" : tone === "accent" ? "text-accent" : "text-fg-subtle",
            )}
          >
            {tone === "ok" && <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" />}
            <span className="truncate">{status}</span>
          </span>
        )}
        <ChevronDown
          className={clsx("size-4 shrink-0 text-fg-subtle transition-transform", !status && "ml-auto", !collapsed && "rotate-180")}
        />
      </button>
      <Collapse open={!collapsed}>
        <div className="border-t border-stroke">{children}</div>
      </Collapse>
    </div>
  );
}

function Integrations() {
  const mail = useMailSettings().data;
  const gcal = useGcalStatus().data;
  const ical = useIcalCalendars().data ?? [];
  const notion = useNotionStatus().data;
  const spotify = useSpotifyStatus().data;
  const youtube = useYoutubeSettings().data;
  const yandex = useYandexStatus().data;
  const discord = useDiscord().data;
  const location = usePrefs((s) => s.location);
  const update = useUpdateStatus().data;

  const connected = (s: { connected: boolean; error: string | null } | undefined, who: string | null | undefined) =>
    !s ? {} : s.error ? { status: "ошибка входа", tone: "warn" as Tone } : s.connected ? { status: who || "подключено", tone: "ok" as Tone } : { status: "не подключено" };
  const mailCount = mail?.accounts.length ?? 0;
  const channels = youtube?.channels.length ?? 0;

  return (
    <div className="flex flex-col gap-2">
      <Group
        id="mail"
        icon={Mail}
        title="Почта"
        {...(mail && (mailCount ? { status: plural(mailCount, ["ящик", "ящика", "ящиков"]), tone: "ok" as Tone } : { status: "не подключена" }))}
      >
        <MailSettings />
      </Group>
      <Group id="gcal" icon={CalendarDays} title="Google Календарь" {...connected(gcal, gcal?.email)}>
        <GcalSettings />
      </Group>
      <Group
        id="ical"
        icon={Link2}
        title="Календари по ссылке (iCal)"
        {...(ical.length ? { status: plural(ical.length, ["календарь", "календаря", "календарей"]), tone: "ok" as Tone } : { status: "нет" })}
      >
        <CalendarSettings />
      </Group>
      <Group id="notion" icon={NotebookPen} title="Notion" {...connected(notion, notion?.workspace)}>
        <NotionSettings />
      </Group>
      <Group id="spotify" icon={Music} title="Spotify" {...connected(spotify, spotify?.user)}>
        <SpotifySettings />
      </Group>
      <Group
        id="yandex"
        icon={House}
        title="Дом с Алисой"
        {...(yandex &&
          (yandex.error
            ? { status: "ошибка", tone: "warn" as Tone }
            : yandex.connected
              ? { status: plural(yandex.lamps, ["лампа", "лампы", "ламп"]), tone: "ok" as Tone }
              : { status: "не подключено" }))}
      >
        <YandexSettings />
      </Group>
      <Group
        id="discord"
        icon={Headphones}
        title="Discord"
        {...(discord &&
          (discord.error
            ? { status: "ошибка", tone: "warn" as Tone }
            : discord.configured
              ? { status: discord.connected ? discord.user?.name || "подключено" : "Discord не запущен", tone: (discord.connected ? "ok" : "off") as Tone }
              : { status: "не подключено" }))}
      >
        <DiscordSettings />
      </Group>
      <Group
        id="youtube"
        icon={TvMinimalPlay}
        title="YouTube"
        {...(youtube?.googleError
          ? { status: "ошибка синхронизации", tone: "warn" as Tone }
          : channels
            ? { status: plural(channels, ["канал", "канала", "каналов"]), tone: "ok" as Tone }
            : { status: "нет каналов" })}
      >
        <YoutubeSettings />
      </Group>
      <Group
        id="weather"
        icon={CloudSun}
        title="Погода"
        {...(location ? { status: location.name, tone: "ok" as Tone } : { status: "город не выбран" })}
      >
        <CityPicker />
      </Group>
      <Group
        id="updates"
        icon={Download}
        title="Обновления"
        {...(update?.available
          ? { status: `доступна ${update.available.version}`, tone: "accent" as Tone }
          : update && { status: `версия ${update.current}` })}
      >
        <UpdateSettings />
      </Group>
    </div>
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
        <WidthRow />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Кнопка на панели задач</h3>
      <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
        <TaskbarButtonRow />
      </div>
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Оформление</h3>
      <AppearanceSection />
      <h3 className="px-1 pt-1 text-[12px] font-medium text-fg-subtle">Интеграции</h3>
      <Integrations />
    </div>
  );
}
