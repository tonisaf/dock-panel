import { describe, expect, it } from "vitest";
import { formatContext, formatSize, unloadsIn } from "./format";

describe("lmstudio format", () => {
  it("formats sizes", () => {
    expect(formatSize(22_069_769_172)).toBe("20.6 ГБ");
    expect(formatSize(84_106_624)).toBe("80 МБ");
  });

  it("formats context windows", () => {
    expect(formatContext(65536)).toBe("64k");
    expect(formatContext(262144)).toBe("256k");
    expect(formatContext(512)).toBe("512");
  });

  it("tells when an idle model unloads", () => {
    const now = 1_000_000_000;
    expect(unloadsIn(null, now, now)).toBeNull();
    expect(unloadsIn(600_000, now - 60_000, now)).toBe("9 мин");
    expect(unloadsIn(600_000, now - 599_000, now)).toBe("меньше минуты");
    expect(unloadsIn(7_200_000, now, now)).toBe("2 ч 0 мин");
  });
});
