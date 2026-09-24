import type { ComponentType } from "react";
import { WeatherWidget } from "./weather/WeatherWidget";
import { MediaWidget } from "./media/MediaWidget";
import { SystemWidget } from "./system/SystemWidget";
import { AiLimitsWidget } from "./ai/AiLimitsWidget";
import { TasksWidget } from "./tasks/TasksWidget";
import { CalendarWidget } from "./calendar/CalendarWidget";
import { PlaylistsWidget } from "./spotify/PlaylistsWidget";
import { VpnWidget } from "./vpn/VpnWidget";
import { PinnedWidget } from "./pinned/PinnedWidget";
import { HomeWidget } from "./home/HomeWidget";

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
  { id: "tasks", title: "Задачи", component: TasksWidget },
  { id: "ai", title: "Лимиты AI", component: AiLimitsWidget },
  { id: "vpn", title: "VPN", component: VpnWidget },
  { id: "home", title: "Дом", component: HomeWidget },
  { id: "system", title: "Система", component: SystemWidget },
];
