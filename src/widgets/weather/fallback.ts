// Fallback providers for networks where Open-Meteo is blocked, converted to
// the Open-Meteo shapes the widget already understands.

import type { WeatherLocation } from "../../lib/prefs";
import type { Weather } from "./api";

// ---- MET Norway (api.met.no locationforecast/2.0/compact) --------------------

interface MetStep {
  /** UTC, on the hour. */
  time: string;
  data: {
    instant: { details: { air_temperature: number; wind_speed: number; relative_humidity: number } };
    next_1_hours?: { summary: { symbol_code: string } };
    next_6_hours?: { summary: { symbol_code: string } };
    next_12_hours?: { summary: { symbol_code: string } };
  };
}

export interface MetForecast {
  properties: { timeseries: MetStep[] };
}

/** MET symbol ("lightrainshowers_day") → the closest WMO code. */
const SYMBOL_TO_WMO: Record<string, number> = {
  clearsky: 0,
  fair: 1,
  partlycloudy: 2,
  cloudy: 3,
  fog: 45,
  lightrain: 61,
  rain: 63,
  heavyrain: 65,
  lightsleet: 66,
  sleet: 67,
  heavysleet: 67,
  lightrainshowers: 80,
  rainshowers: 81,
  heavyrainshowers: 82,
  lightsleetshowers: 80,
  sleetshowers: 81,
  heavysleetshowers: 82,
  lightsnow: 71,
  snow: 73,
  heavysnow: 75,
  lightsnowshowers: 85,
  snowshowers: 85,
  heavysnowshowers: 86,
};

export function symbolToWmo(symbol: string | undefined) {
  if (!symbol) return 3;
  const base = symbol.replace(/_(day|night|polartwilight)$/, "");
  if (base.includes("thunder")) return 95;
  return SYMBOL_TO_WMO[base] ?? 3;
}

/** Whether the sun is above the horizon at a place and moment (±1 minute is plenty for an icon). */
export function isDaylight(lat: number, lon: number, at: Date) {
  const rad = Math.PI / 180;
  const days = at.getTime() / 86_400_000 - 10_957.5; // since J2000.0
  const anomaly = (357.529 + 0.98560028 * days) * rad;
  const meanLon = 280.459 + 0.98564736 * days;
  const eclLon = (meanLon + 1.915 * Math.sin(anomaly) + 0.02 * Math.sin(2 * anomaly)) * rad;
  const obliquity = (23.439 - 0.00000036 * days) * rad;
  const declination = Math.asin(Math.sin(obliquity) * Math.sin(eclLon));
  const rightAscension = Math.atan2(Math.cos(obliquity) * Math.sin(eclLon), Math.cos(eclLon));
  const siderealDeg = (280.46061837 + 360.98564736629 * days) % 360;
  const hourAngle = (siderealDeg + lon) * rad - rightAscension;
  const altitude = Math.asin(
    Math.sin(lat * rad) * Math.sin(declination) + Math.cos(lat * rad) * Math.cos(declination) * Math.cos(hourAngle),
  );
  return altitude > -0.833 * rad; // refraction + solar disc radius
}

/** Australian BoM apparent temperature (no solar radiation), as a stand-in for Open-Meteo's. */
export function apparentTemperature(temp: number, humidity: number, wind: number) {
  const vapour = (humidity / 100) * 6.105 * Math.exp((17.27 * temp) / (237.7 + temp));
  return temp + 0.33 * vapour - 0.7 * wind - 4;
}

const pad = (n: number) => String(n).padStart(2, "0");
/** "2026-09-24T15:00" in this PC's time zone, like Open-Meteo's `timezone=auto` strings. */
const localStamp = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;

/**
 * Times come out in the PC's time zone: MET doesn't report the place's zone,
 * and the widget's city is normally where the PC is.
 */
export function fromMetNo(raw: MetForecast, lat: number, lon: number, now = new Date()): Weather {
  const steps = raw.properties.timeseries.map((s) => {
    const at = new Date(s.time);
    const { air_temperature, wind_speed, relative_humidity } = s.data.instant.details;
    const symbol = (s.data.next_1_hours ?? s.data.next_6_hours ?? s.data.next_12_hours)?.summary.symbol_code;
    // Icons describe the coming hour, so judge day or night at its middle.
    const isDay = isDaylight(lat, lon, new Date(at.getTime() + 30 * 60_000)) ? 1 : 0;
    return { at, time: localStamp(at), temp: air_temperature, wind: wind_speed, humidity: relative_humidity, code: symbolToWmo(symbol), isDay };
  });
  if (steps.length === 0) throw new Error("met.no: empty forecast");

  // The step for the current hour; the series starts at it or just before.
  const current = [...steps].reverse().find((s) => s.at <= now) ?? steps[0];
  const today = localStamp(now).slice(0, 10);
  // MET has no past hours, so today's range covers what is left of the day.
  const todayTemps = steps.filter((s) => s.time.startsWith(today) && s.at >= current.at).map((s) => s.temp);
  const range = todayTemps.length ? todayTemps : [current.temp];

  return {
    current: {
      time: current.time,
      temperature_2m: current.temp,
      apparent_temperature: apparentTemperature(current.temp, current.humidity, current.wind),
      weather_code: current.code,
      is_day: current.isDay,
      wind_speed_10m: current.wind,
      relative_humidity_2m: Math.round(current.humidity),
    },
    hourly: {
      time: steps.map((s) => s.time),
      temperature_2m: steps.map((s) => s.temp),
      weather_code: steps.map((s) => s.code),
      is_day: steps.map((s) => s.isDay),
    },
    daily: { temperature_2m_max: [Math.max(...range)], temperature_2m_min: [Math.min(...range)] },
  };
}

// ---- OpenStreetMap Nominatim (search, format=jsonv2, addressdetails=1) ------

export interface NominatimPlace {
  name?: string;
  display_name: string;
  lat: string;
  lon: string;
  address?: { state?: string; region?: string; country?: string };
}

export function fromNominatim(places: NominatimPlace[]): WeatherLocation[] {
  return places.map((p) => ({
    name: p.name || p.display_name.split(",")[0],
    detail: [p.address?.state ?? p.address?.region, p.address?.country].filter(Boolean).join(", "),
    latitude: Number(p.lat),
    longitude: Number(p.lon),
  }));
}
