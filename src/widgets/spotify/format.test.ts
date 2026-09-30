import { describe, expect, it } from "vitest";
import { formatListened, peakHour, shortDay } from "./format";

describe("formatListened", () => {
  it("shows minutes, then hours and minutes", () => {
    expect(formatListened(0)).toBe("0 мин");
    expect(formatListened(20_000)).toBe("< 1 мин");
    expect(formatListened(42 * 60_000 + 59_000)).toBe("42 мин");
    expect(formatListened(3 * 3_600_000 + 5 * 60_000)).toBe("3 ч 05 мин");
  });

  it("drops the minutes once there are many hours", () => {
    expect(formatListened(112 * 3_600_000 + 30 * 60_000)).toBe("112 ч");
  });
});

describe("peakHour", () => {
  it("finds the busiest hour", () => {
    const hours = Array(24).fill(0);
    hours[9] = 5;
    hours[21] = 12;
    expect(peakHour(hours)).toBe(21);
  });

  it("is null without any listening", () => {
    expect(peakHour(Array(24).fill(0))).toBeNull();
  });
});

describe("shortDay", () => {
  it("names the day and month", () => {
    expect(shortDay("2026-09-30")).toBe("30 сен");
    expect(shortDay("2026-01-05")).toBe("5 янв");
  });

  it("leaves other text alone", () => {
    expect(shortDay("today")).toBe("today");
  });
});
