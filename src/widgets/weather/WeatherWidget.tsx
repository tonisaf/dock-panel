import { CloudSun, Droplets, MapPin, Wind } from "lucide-react";
import { Card } from "../../components/Card";
import { usePrefs } from "../../lib/prefs";
import { usePanelStore } from "../../store";
import { describe, formatTemp, upcomingHours, useWeather } from "./api";

export function WeatherWidget() {
  const location = usePrefs((s) => s.location);
  const setTab = usePanelStore((s) => s.setTab);
  const { data, isPending, isError } = useWeather();

  if (!location) {
    return (
      <Card title="Погода" icon={CloudSun}>
        <button
          onClick={() => setTab("settings")}
          className="flex items-center gap-2 rounded-lg border border-stroke px-3 py-2 text-[13px] text-fg-muted hover:bg-ink/8 hover:text-fg"
        >
          <MapPin className="size-4" /> Указать город
        </button>
      </Card>
    );
  }

  if (isPending || isError || !data) {
    return (
      <Card title={`Погода · ${location.name}`} icon={CloudSun}>
        <p className="text-[12px] text-fg-subtle">{isError ? "Не удалось загрузить прогноз" : "Загрузка…"}</p>
      </Card>
    );
  }

  const now = describe(data.current.weather_code, data.current.is_day === 1);
  const NowIcon = now.icon;
  const hours = upcomingHours(data, 6);

  return (
    <Card>
      <div className="flex items-start justify-between">
        <div>
          <div className="flex items-center gap-1 text-[12px] font-medium text-fg-muted">
            <MapPin className="size-3.5" /> {location.name}
          </div>
          <div className="mt-1 flex items-center gap-3">
            <span className="font-display text-[40px] leading-none font-semibold tabular-nums">
              {formatTemp(data.current.temperature_2m)}
            </span>
            <NowIcon className="size-9 text-accent" strokeWidth={1.6} />
          </div>
          <div className="mt-1.5 text-[13px]">{now.label}</div>
        </div>
        <div className="flex flex-col items-end gap-1 text-[12px] text-fg-muted">
          <span>
            ↑ {formatTemp(data.daily.temperature_2m_max[0])} ↓ {formatTemp(data.daily.temperature_2m_min[0])}
          </span>
          <span>ощущается {formatTemp(data.current.apparent_temperature)}</span>
          <span className="flex items-center gap-1">
            <Wind className="size-3.5" /> {Math.round(data.current.wind_speed_10m)} м/с
          </span>
          <span className="flex items-center gap-1">
            <Droplets className="size-3.5" /> {data.current.relative_humidity_2m}%
          </span>
        </div>
      </div>

      <div className="mt-3 grid grid-cols-6 gap-1 border-t border-stroke pt-3">
        {hours.map((h) => {
          const Icon = describe(h.code, h.isDay).icon;
          return (
            <div key={h.hour} className="flex flex-col items-center gap-1.5 text-[12px]">
              <span className="text-fg-subtle">{h.hour}</span>
              <Icon className="size-[18px] text-fg-muted" strokeWidth={1.8} />
              <span className="tabular-nums">{formatTemp(h.temperature)}</span>
            </div>
          );
        })}
      </div>
    </Card>
  );
}
