import { describe, expect, it } from "vitest";
import {
  DEFAULT_BINDINGS,
  bindingOf,
  conflictOf,
  formatShortcut,
  recordKeyPress,
  shortcutProblem,
  type KeyPress,
} from "./hotkeys";

function keys(
  code: string,
  mods: Partial<Omit<KeyPress, "code">> = {},
): KeyPress {
  return {
    code,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...mods,
  };
}

describe("recordKeyPress", () => {
  it("waits while only modifiers are held", () => {
    expect(recordKeyPress(keys("ControlLeft", { ctrlKey: true }))).toEqual({
      kind: "modifiers",
      text: "Ctrl+",
    });
    expect(
      recordKeyPress(keys("ShiftRight", { ctrlKey: true, shiftKey: true })),
    ).toEqual({ kind: "modifiers", text: "Ctrl+Shift+" });
  });

  it("writes modifiers in a fixed order, then the key", () => {
    expect(
      recordKeyPress(
        keys("KeyI", { metaKey: true, altKey: true, ctrlKey: true }),
      ),
    ).toEqual({ kind: "shortcut", value: "Ctrl+Alt+Win+KeyI" });
    expect(recordKeyPress(keys("F13"))).toEqual({
      kind: "shortcut",
      value: "F13",
    });
  });

  it("refuses typing keys without Ctrl, Alt or Win, and unsupported keys", () => {
    expect(recordKeyPress(keys("KeyT"))).toEqual({
      kind: "invalid",
      problem: "needsModifier",
      text: "KeyT",
    });
    expect(recordKeyPress(keys("Space", { shiftKey: true }))).toMatchObject({
      problem: "needsModifier",
    });
    expect(recordKeyPress(keys("CapsLock", { ctrlKey: true }))).toMatchObject({
      problem: "unsupportedKey",
    });
    expect(recordKeyPress(keys("Escape", { altKey: true }))).toMatchObject({
      problem: "unsupportedKey",
    });
  });
});

describe("shortcutProblem", () => {
  it("matches the core's rules", () => {
    expect(shortcutProblem("Ctrl+Alt+KeyI")).toBeNull();
    expect(shortcutProblem("Alt+Digit1")).toBeNull();
    expect(shortcutProblem("Pause")).toBeNull();
    expect(shortcutProblem("Shift+Insert")).toBeNull();
    expect(shortcutProblem("Numpad5")).toBe("needsModifier");
    expect(shortcutProblem("Ctrl+MediaPlayPause")).toBe("unsupportedKey");
    for (const value of Object.values(DEFAULT_BINDINGS)) {
      expect(shortcutProblem(value ?? "")).toBeNull();
    }
  });
});

describe("formatShortcut", () => {
  it("shows keys as printed on the keyboard", () => {
    expect(formatShortcut("Ctrl+Alt+KeyI")).toBe("Ctrl + Alt + I");
    expect(formatShortcut("Shift+Win+Digit5")).toBe("Shift + Win + 5");
    expect(formatShortcut("Ctrl+Numpad1")).toBe("Ctrl + Num 1");
    expect(formatShortcut("Alt+ArrowUp")).toBe("Alt + ↑");
    expect(formatShortcut("ScrollLock")).toBe("Scroll Lock");
    expect(formatShortcut("Ctrl+Alt+")).toBe("Ctrl + Alt");
  });
});

describe("bindings", () => {
  const settings = {
    bindings: { toggleBoth: null, pushToTalk: "Ctrl+Alt+KeyO" },
    soundCues: true,
  };

  it("uses the default for a missing action and null for none", () => {
    expect(bindingOf(settings, "toggleIncoming")).toBe("Ctrl+Alt+KeyI");
    expect(bindingOf(settings, "toggleBoth")).toBeNull();
  });

  it("finds the other action using a shortcut", () => {
    expect(conflictOf(settings, "toggleIncoming", "Ctrl+Alt+KeyO")).toBe(
      "toggleOutgoing",
    );
    expect(conflictOf(settings, "toggleOutgoing", "Ctrl+Alt+KeyO")).toBe(
      "pushToTalk",
    );
    expect(conflictOf(settings, "toggleIncoming", "Ctrl+Alt+KeyI")).toBeNull();
    expect(conflictOf(settings, "toggleIncoming", "F9")).toBeNull();
  });
});
