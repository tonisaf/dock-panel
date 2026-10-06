import { describe, expect, it } from "vitest";
import { searchApps } from "./appSearch";

const apps = ["Telegram", "Terminal", "Visual Studio Code", "Visual Studio Installer", "Windows PowerShell", "Блокнот"].map((name) => ({ id: name, name }));
const names = (q: string) => searchApps(apps, q, {}).map((a) => a.name);
describe("app search", () => {
  it("corrects Russian and English keyboard layouts", () => {
    expect(names("еу")).toContain("Telegram");
    expect(names("ЕУДУП")[0]).toBe("Telegram");
    expect(names(" ,kjryjn ")[0]).toBe("Блокнот");
  });
  it("matches multiple word prefixes in either order", () => {
    expect(names("vis code")).toEqual(["Visual Studio Code"]);
    expect(names("code vis")).toEqual(["Visual Studio Code"]);
    expect(names("  pow   win ")).toEqual(["Windows PowerShell"]);
  });
  it("corrects each token independently", () => {
    expect(names("мшы code")).toEqual(["Visual Studio Code"]);
    expect(names("мшы сщву")).toEqual(["Visual Studio Code"]);
  });
  it("requires every token and does not reuse a name word", () => {
    expect(names("vis absent")).toEqual([]);
    expect(names("visual visual")).toEqual([]);
    expect(names("vis stu code")).toEqual(["Visual Studio Code"]);
  });
  it("keeps initials and fuzzy single word matching", () => {
    expect(names("vsc")[0]).toBe("Visual Studio Code");
    expect(names("tlgrm")).toContain("Telegram");
  });
  it("uses frequency and recency for similarly relevant matches", () => {
    const now = 1_800_000_000_000;
    expect(searchApps(apps, "te", { Terminal: { count: 12, last: now } }, now)[0].name).toBe("Terminal");
    expect(searchApps(apps, "te", { Terminal: { count: 100, last: now - 90 * 86_400_000 }, Telegram: { count: 3, last: now } }, now)[0].name).toBe("Telegram");
  });
  it("does not let launch frequency overpower exact or direct matches", () => {
    const now = Date.now();
    const candidates = [{ id: "exact", name: "Code" }, { id: "frequent", name: "Code Editor" }];
    expect(searchApps(candidates, "code", { frequent: { count: 10000, last: now } }, now)[0].id).toBe("exact");
  });
  it("handles empty queries and limits results", () => {
    expect(names("  ")).toEqual([]);
    expect(searchApps(Array.from({ length: 80 }, (_, i) => ({ id: String(i), name: `Tool ${i}` })), "tool", {})).toHaveLength(50);
  });
});
