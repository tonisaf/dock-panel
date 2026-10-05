import { describe, expect, it } from "vitest";
import { health, shortStatus, stateTone, summary } from "./format";

describe("docker format", () => {
  it("tones states", () => {
    expect(stateTone("running")).toBe("ok");
    expect(stateTone("restarting")).toBe("warn");
    expect(stateTone("paused")).toBe("warn");
    expect(stateTone("exited")).toBe("off");
    expect(stateTone("dead")).toBe("off");
  });

  it("reads the health check from the status", () => {
    expect(health("Up 3 hours (healthy)")).toBe("healthy");
    expect(health("Up 3 hours (unhealthy)")).toBe("unhealthy");
    expect(health("Up 5 seconds (health: starting)")).toBe("starting");
    expect(health("Up 3 hours")).toBeNull();
  });

  it("shortens statuses", () => {
    expect(shortStatus("Up 3 hours")).toBe("3 ч");
    expect(shortStatus("Up 3 hours (healthy)")).toBe("3 ч");
    expect(shortStatus("Up About an hour")).toBe("1 ч");
    expect(shortStatus("Up Less than a second")).toBe("<1 сек");
    expect(shortStatus("Exited (0) 2 days ago")).toBe("2 дн назад");
    expect(shortStatus("Exited (137) 5 minutes ago")).toBe("5 мин назад");
    expect(shortStatus("Created")).toBe("Created");
  });

  it("summarises", () => {
    expect(summary(3, 7)).toBe("3 из 7");
  });
});
