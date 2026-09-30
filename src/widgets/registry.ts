import type { ComponentType } from "react";
import { WeatherWidget } from "./weather/WeatherWidget";
import { MediaWidget } from "./media/MediaWidget";
import { SystemWidget } from "./system/SystemWidget";
import { AiLimitsWidget } from "./ai/AiLimitsWidget";
import { TasksWidget } from "./tasks/TasksWidget";
import { CalendarWidget } from "./calendar/CalendarWidget";
import { PlaylistsWidget } from "./spotify/PlaylistsWidget";
import { HistoryWidget } from "./spotify/HistoryWidget";
import { VpnWidget } from "./vpn/VpnWidget";
import { PinnedWidget } from "./pinned/PinnedWidget";
import { MailWidget } from "./mail/MailWidget";
import { YoutubeWidget } from "./youtube/YoutubeWidget";
import { HomeWidget } from "./home/HomeWidget";
import { MonitorsWidget } from "./monitors/MonitorsWidget";
import { AgentsWidget } from "./agents/AgentsWidget";
import { AskWidget } from "./ask/AskWidget";
import { DiscordWidget } from "./discord/DiscordWidget";
import { PomodoroWidget } from "./pomodoro/PomodoroWidget";
import { NotesWidget } from "./notes/NotesWidget";

export interface WidgetDef {
  id: string;
  title: string;
  component: ComponentType;
}

/**
 * Home-tab widgets in display order. A new widget is a folder under
 * `src/widgets/` plus one entry here.
 */
export const WIDGETS: WidgetDef[] = [
  { id: "pinned", title: "Закреплённые", component: PinnedWidget },
  { id: "weather", title: "Погода", component: WeatherWidget },
  { id: "calendar", title: "Календарь", component: CalendarWidget },
  { id: "media", title: "Сейчас играет", component: MediaWidget },
  { id: "playlists", title: "Плейлисты", component: PlaylistsWidget },
  { id: "history", title: "Моя музыка", component: HistoryWidget },
  { id: "mail", title: "Почта", component: MailWidget },
  { id: "youtube", title: "YouTube", component: YoutubeWidget },
  { id: "discord", title: "Discord", component: DiscordWidget },
  { id: "tasks", title: "Задачи", component: TasksWidget },
  { id: "notes", title: "Заметки", component: NotesWidget },
  { id: "ai", title: "Лимиты AI", component: AiLimitsWidget },
  { id: "agents", title: "Агенты", component: AgentsWidget },
  { id: "ask", title: "Спросить Claude", component: AskWidget },
  { id: "pomodoro", title: "Помодоро", component: PomodoroWidget },
  { id: "vpn", title: "VPN", component: VpnWidget },
  { id: "home", title: "Дом", component: HomeWidget },
  { id: "monitors", title: "Мониторы", component: MonitorsWidget },
  { id: "system", title: "Система", component: SystemWidget },
];
