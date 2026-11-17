import { describe, expect, it } from "vitest";
import { createTranslator } from "../i18n/translate";
import type { AudioApp } from "./api";
import {
  NO_APP_VALUE,
  appOptions,
  appSelectValue,
  sameProcess,
  withoutExe,
} from "./audioApps";

const { t } = createTranslator("en");

const chrome: AudioApp = {
  processName: "chrome.exe",
  displayName: "Google Chrome",
  pid: 1100,
  sessionPids: [1102],
  active: true,
};

describe("audio app helpers", () => {
  it("compares executable names without case", () => {
    expect(sameProcess("Discord.exe", "discord.EXE")).toBe(true);
    expect(sameProcess("Discord.exe", "Discord")).toBe(false);
  });

  it("drops the .exe extension for display", () => {
    expect(withoutExe("Discord.exe")).toBe("Discord");
    expect(withoutExe("GAME.EXE")).toBe("GAME");
    expect(withoutExe("weird")).toBe("weird");
  });

  it("offers a placeholder only while no app is chosen", () => {
    expect(appOptions({ apps: [chrome], savedName: null, t })).toEqual([
      { value: NO_APP_VALUE, label: "Choose an app…" },
      { value: "chrome.exe", label: "Google Chrome · playing" },
    ]);
    expect(appOptions({ apps: [chrome], savedName: "CHROME.exe", t })).toEqual([
      { value: "chrome.exe", label: "Google Chrome · playing" },
    ]);
  });

  it("keeps the saved app while the list is loading", () => {
    expect(appOptions({ apps: null, savedName: "Discord.exe", t })).toEqual([
      { value: "Discord.exe", label: "Discord" },
    ]);
  });

  it("selects the listed spelling of the saved app", () => {
    expect(appSelectValue([chrome], "Chrome.EXE")).toBe("chrome.exe");
    expect(appSelectValue([chrome], "Zoom.exe")).toBe("Zoom.exe");
    expect(appSelectValue(null, null)).toBe(NO_APP_VALUE);
  });
});
