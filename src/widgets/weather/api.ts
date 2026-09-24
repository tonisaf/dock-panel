import { useQuery } from "@tanstack/react-query";
import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudMoon,
  CloudRain,
  CloudSnow,
  CloudSun,
  Moon,
  Sun,
  type LucideIcon,
} from "lucide-react";
import { usePrefs, type WeatherLocation } from "../../lib/prefs";

/** Open-Meteo: free, no API key. https://open-meteo.com/en/docs */
const FORECAST_URL = "https://api.open-meteo.com/v1/forecast";
const GEOCODE_URL = "https://geocoding-api.open-meteo.com/v1/search";

export interface Weather {
  current: {
    time: string;
    temperature_2m: number;
    apparent_temperature: number;
    weather_code: number;
    is_day: number;
    wind_speed_10m: number;
    relative_humidity_2m: number;
  };
  hourly: { time: string[]; temperature_2m: number[]; weather_code: number[]; is_day: number[] };
  daily: { temperature_2m_max: number[]; temperature_2m_min: number[] };
}

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

export function useWeather() {
  const location = usePrefs((s) => s.location);
  return useQuery({
    queryKey: ["weather", location?.latitude, location?.longitude],
    enabled: !!location,
    staleTime: 10 * 60_000,
    refetchInterval: 15 * 60_000,
    queryFn: () => {
      const params = new URLSearchParams({
        latitude: String(location!.latitude),
        longitude: String(location!.longitude),
        current: "temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,relative_humidity_2m",
        hourly: "temperature_2m,weather_code,is_day",
        daily: "temperature_2m_max,temperature_2m_min",
        timezone: "auto",
        forecast_days: "2",
        wind_speed_unit: "ms",
      });
      return getJson<Weather>(`${FORECAST_URL}?${params}`);
    },
  });
}

interface GeoResult {
  name: string;
  latitude: number;
  longitude: number;
  country?: string;
  admin1?: string;
}

export async function searchCities(query: string): Promise<WeatherLocation[]> {
  const params = new URLSearchParams({ name: query, count: "6", language: "ru", format: "json" });
  const data = await getJson<{ results?: GeoResult[] }>(`${GEOCODE_URL}?${params}`);
  return (data.results ?? []).map((r) => ({
    name: r.name,
    detail: [r.admin1, r.country].filter(Boolean).join(", "),
    latitude: r.latitude,
    longitude: r.longitude,
  }));
}

/** The next `count` hourly slots after the current hour. */
export function upcomingHours(w: Weather, count: number) {
  const currentHour = w.current.time.slice(0, 13); // "2026-09-24T15"
  const start = w.hourly.time.findIndex((t) => t.slice(0, 13) === currentHour) + 1;
  return w.hourly.time.slice(start, start + count).map((time, i) => ({
    hour: time.slice(11, 16),
    temperature: w.hourly.temperature_2m[start + i],
    code: w.hourly.weather_code[start + i],
    isDay: w.hourly.is_day[start + i] === 1,
  }));
}

/** WMO weather interpretation codes. */
export function describe(code: number, isDay = true): { label: string; icon: LucideIcon } {
  if (code === 0) return { label: "Ясно", icon: isDay ? Sun : Moon };
  if (code === 1) return { label: "Преимущественно ясно", icon: isDay ? CloudSun : CloudMoon };
  if (code === 2) return { label: "Переменная облачность", icon: isDay ? CloudSun : CloudMoon };
  if (code === 3) return { label: "Пасмурно", icon: Cloud };
  if (code === 45 || code === 48) return { label: "Туман", icon: CloudFog };
  if (code >= 51 && code <= 57) return { label: "Морось", icon: CloudDrizzle };
  if (code >= 61 && code <= 67) return { label: "Дождь", icon: CloudRain };
  if (code >= 71 && code <= 77) return { label: "Снег", icon: CloudSnow };
  if (code >= 80 && code <= 82) return { label: "Ливень", icon: CloudRain };
  if (code === 85 || code === 86) return { label: "Снегопад", icon: CloudSnow };
  if (code >= 95) return { label: "Гроза", icon: CloudLightning };
  return { label: "—", icon: Cloud };
}

export const formatTemp = (t: number) => `${Math.round(t)}°`;
