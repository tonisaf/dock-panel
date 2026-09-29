import { describe, expect, it } from "vitest";
import { prettyAccelerator, toAccelerator } from "./accelerator";

const key = (code: string, mods: Partial<KeyboardEvent> = {}) => ({ code, ...mods }) as KeyboardEvent;

describe("toAccelerator", () => {
  it("joins modifiers and the key", () => {
    expect(toAccelerator(key("Space", { ctrlKey: true }))).toBe("Ctrl+Space");
    expect(toAccelerator(key("KeyK", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+K");
    expect(toAccelerator(key("Digit1", { altKey: true, metaKey: true }))).toBe("Alt+Super+1");
  });

  it("waits while only modifiers are held", () => {
    expect(toAccelerator(key("ControlLeft", { ctrlKey: true }))).toBeNull();
  });

  it("refuses a bare key except function keys", () => {
    expect(toAccelerator(key("KeyA"))).toBeNull();
    expect(toAccelerator(key("F12"))).toBe("F12");
  });
});

describe("prettyAccelerator", () => {
  it("spells Super as Win", () => {
    expect(prettyAccelerator("Super+Space")).toBe("Win + Space");
  });
});
