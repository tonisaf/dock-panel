const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight"]);

/** KeyboardEvent -> accelerator ("Ctrl+Alt+A"), or null while only modifiers are held. */
export function toAccelerator(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  // Bare keys would swallow normal typing; function keys are the exception.
  if (mods.length === 0 && !/^F\d{1,2}$/.test(key)) return null;
  return [...mods, key].join("+");
}

export const prettyAccelerator = (accel: string) => accel.replace(/Super/g, "Win").replace(/\+/g, " + ");
