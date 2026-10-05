import { useEffect, useState, type ReactNode } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import clsx from "clsx";
import { CityPicker } from "../components/CityPicker";
import { NotionSettings } from "../components/NotionSettings";
import { LmStudioSettings, useLmStudioStatus } from "../components/LmStudioSettings";
import { OpenClawSettings, useOpenClawStatus } from "../components/OpenClawSettings";
import { CalendarSettings } from "../components/CalendarSettings";
import { SpotifySettings } from "../components/SpotifySettings";
import { UpdateSettings } from "../components/UpdateSettings";
import {
  MAX_WIDTH,
  MIN_WIDTH,
  WIDGET_MAX,
  WIDGET_MIN,
  columnsIn,
  panelWidthFor,
  usePanelSettings,
  usePanelWidth,
  widthPresets,
  type Edge,
} from "../lib/panelWidth";
import { usePrefs, type SearchEngine, type ThemeMode } from "../lib/prefs";
import { Toggle } from "../components/Toggle";
import { Slider } from "../components/Slider";
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
  Headphones,
  House,
  Link2,
  Mail,
  Music,
  NotebookPen,
  Bot,
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
  const widgetWidth = usePrefs((s) => s.widgetWidth);
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
        {widthPresets(widgetWidth).map((p) => (
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

function SearchEngineRow() {
  const engine = usePrefs((s) => s.searchEngine);
  const setEngine = usePrefs((s) => s.setSearchEngine);
  return (
    <Row label="Поиск в интернете" hint="Последняя строка результатов поиска">
      <Segmented<SearchEngine>
        value={engine}
        onChange={setEngine}
        options={[
          { value: "google", label: "Google" },
          { value: "yandex", label: "Яндекс" },
          { value: "duckduckgo", label: "DuckDuckGo" },
        ]}
      />
    </Row>
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
  const openclaw = useOpenClawStatus().data;
  const lmstudio = useLmStudioStatus().data;
  const spotify = useSpotifyStatus().data;
  const youtube = useYoutubeSettings().data;
  const yandex = useYandexStatus().data;
  const discord = useDiscord().data;
  const location = usePrefs((s) => s.location);

  const connected = (s: { connected: boolean; error: string | null } | undefined, who: string | null | undefined) =>
    !s ? {} : s.error ? { status: "ошибка входа", tone: "warn" as Tone } : s.connected ? { status: who || "подключено", tone: "ok" as Tone } : { status: "не подключено" };
  const mailCount = mail?.accounts.length ?? 0;
  const channels = youtube?.channels.length ?? 0;

  const configured: Record<string, boolean> = {
    mail: mailCount > 0,
    gcal: !!gcal?.connected,
    ical: ical.length > 0,
    notion: !!notion?.connected,
    openclaw: !!openclaw?.connected,
    lmstudio: !!lmstudio?.reachable,
    spotify: !!spotify?.connected,
    yandex: !!yandex?.connected,
    discord: !!discord?.configured,
    youtube: channels > 0,
    weather: !!location,
  };
  const groups = [
    <Group key="mail"
        id="mail"
        icon={Mail}
        title="Почта"
        {...(mail && (mailCount ? { status: plural(mailCount, ["ящик", "ящика", "ящиков"]), tone: "ok" as Tone } : { status: "не подключена" }))}
      >
        <MailSettings />
      </Group>,
    <Group key="gcal" id="gcal" icon={CalendarDays} title="Google Календарь" {...connected(gcal, gcal?.email)}>
        <GcalSettings />
      </Group>,
    <Group key="ical"
        id="ical"
        icon={Link2}
        title="Календари по ссылке (iCal)"
        {...(ical.length ? { status: plural(ical.length, ["календарь", "календаря", "календарей"]), tone: "ok" as Tone } : { status: "нет" })}
      >
        <CalendarSettings />
      </Group>,
    <Group key="notion" id="notion" icon={NotebookPen} title="Notion" {...connected(notion, notion?.workspace)}>
        <NotionSettings />
      </Group>,
    <Group key="openclaw"
        id="openclaw"
        icon={Bot}
        title="OpenClaw"
        {...(openclaw &&
          (!openclaw.connected
            ? { status: "не подключено" }
            : openclaw.reachable
              ? { status: "Gateway отвечает", tone: "ok" as Tone }
              : { status: "не отвечает", tone: "warn" as Tone }))}
      >
        <OpenClawSettings />
      </Group>,
    <Group key="lmstudio"
        id="lmstudio"
        icon={Bot}
        title="LM Studio"
        {...(lmstudio &&
          (lmstudio.reachable
            ? { status: lmstudio.model ?? "сервер отвечает", tone: "ok" as Tone }
            : { status: "не запущен", tone: "warn" as Tone }))}
      >
        <LmStudioSettings />
      </Group>,
    <Group key="spotify" id="spotify" icon={Music} title="Spotify" {...connected(spotify, spotify?.user)}>
        <SpotifySettings />
      </Group>,
    <Group key="yandex"
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
      </Group>,
    <Group key="discord"
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
      </Group>,
    <Group key="youtube"
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
      </Group>,
    <Group key="weather"
        id="weather"
        icon={CloudSun}
        title="Погода"
        {...(location ? { status: location.name, tone: "ok" as Tone } : { status: "город не выбран" })}
      >
        <CityPicker />
      </Group>,
  ];
  // Stable within each group: keep the familiar service order.
  groups.sort((a, b) => Number(configured[b.props.id]) - Number(configured[a.props.id]));
  return <div className="flex flex-col gap-2">{groups}</div>;
}

/** Widget width, in the panel and on the desktop. The panel keeps its number of columns. */
function WidgetWidthRow() {
  const { width, setWidth } = usePanelWidth();
  const { widgetWidth, setWidgetWidth } = usePrefs();
  const [draft, setDraft] = useState<number | null>(null);
  const commit = (next: number) => {
    const columns = columnsIn(width, widgetWidth);
    setWidgetWidth(next);
    setWidth(panelWidthFor(columns, next), true).catch(console.error);
  };
  return (
    <Row label="Ширина виджетов" hint={`${draft ?? widgetWidth} px · в панели и на рабочем столе`}>
      <div className="w-36">
        <Slider
          label="Ширина виджетов"
          value={widgetWidth}
          min={WIDGET_MIN}
          max={WIDGET_MAX}
          step={10}
          onDraft={setDraft}
          onCommit={commit}
        />
      </div>
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
      <WidgetWidthRow />
    </div>
  );
}

function DesktopSection() {
  const { deskOpacity, setDeskOpacity, deskBlur, setDeskBlur } = usePrefs();
  return (
    <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
      <Row label="Непрозрачность фона" hint={`${deskOpacity}% · 0 — только текст на обоях`}>
        <div className="w-36">
          <Slider
            label="Непрозрачность фона виджетов на рабочем столе"
            value={deskOpacity}
            min={0}
            max={100}
            step={5}
            onDraft={(v) => v != null && setDeskOpacity(v)}
            onCommit={setDeskOpacity}
          />
        </div>
      </Row>
      <Row label="Размытие обоев под виджетом">
        <Toggle on={deskBlur} onChange={setDeskBlur} />
      </Row>
    </div>
  );
}

const SETTINGS_SECTIONS = [
  { id: "panel", label: "Панель" },
  { id: "appearance", label: "Оформление" },
  { id: "integrations", label: "Интеграции" },
  { id: "updates", label: "Обновления" },
] as const;

export function SettingsTab() {
  const [section, setSection] = useState<(typeof SETTINGS_SECTIONS)[number]["id"]>("panel");
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
      <nav aria-label="Разделы настроек" className="grid grid-cols-2 gap-1 rounded-xl border border-stroke bg-surface p-1 min-[700px]:grid-cols-4">
        {SETTINGS_SECTIONS.map(({ id, label }) => (
          <button
            key={id}
            type="button"
            aria-pressed={section === id}
            onClick={() => setSection(id)}
            className={clsx(
              "rounded-lg px-2 py-2 text-[13px] font-medium transition-colors focus-visible:outline-2 focus-visible:outline-accent",
              section === id ? "bg-ink/10 text-fg" : "text-fg-muted hover:bg-ink/5 hover:text-fg",
            )}
          >
            {label}
          </button>
        ))}
      </nav>
      {section === "panel" && (
        <>
          <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
            <Row label="Запускать вместе с Windows">
              <Toggle on={autostart} onChange={toggleAutostart} />
            </Row>
            <ShortcutRow />
            <EdgeRow />
            <SearchEngineRow />
            <WidthRow />
          </div>
          <h3 className="px-1 text-[12px] font-medium text-fg-subtle">Кнопка на панели задач</h3>
          <div className="divide-y divide-stroke rounded-2xl border border-stroke bg-surface">
            <TaskbarButtonRow />
          </div>
        </>
      )}
      {section === "appearance" && (
        <>
          <AppearanceSection />
          <h3 className="px-1 text-[12px] font-medium text-fg-subtle">Виджеты на рабочем столе</h3>
          <DesktopSection />
        </>
      )}
      {section === "integrations" && <Integrations />}
      {section === "updates" && (
        <div className="rounded-2xl border border-stroke bg-surface">
          <UpdateSettings />
        </div>
      )}
    </div>
  );
}
