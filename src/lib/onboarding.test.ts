import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioDevice } from "./api";
import {
  ONBOARDING_STEPS,
  loadStartup,
  onboardingMode,
  preferredCable,
  virtualCableOutputs,
} from "./onboarding";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

function device(
  id: string,
  name: string,
  kind: AudioDevice["kind"],
  isVirtualCable: boolean,
): AudioDevice {
  return { id, name, kind, isDefault: false, isVirtualCable };
}

const SPEAKERS = device("speakers", "Speakers (Realtek)", "output", false);
const CABLE_OUTPUT = device(
  "cable-out",
  "CABLE Output (VB-Audio Virtual Cable)",
  "input",
  true,
);
const VOICEMEETER = device(
  "vm",
  "VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)",
  "output",
  true,
);
const CABLE_INPUT = device(
  "cable-in",
  "CABLE Input (VB-Audio Virtual Cable)",
  "output",
  true,
);

describe("onboardingMode", () => {
  it("shows the full setup on a first launch (no key yet)", () => {
    expect(
      onboardingMode({ onboardingCompleted: false }, { hasKey: false }),
    ).toBe("full");
  });

  it("shows the light setup to existing users who already have a key", () => {
    // A settings file migrated from version 4 has `onboardingCompleted: false`.
    expect(
      onboardingMode({ onboardingCompleted: false }, { hasKey: true }),
    ).toBe("light");
    expect(ONBOARDING_STEPS.light).toEqual(["privacy", "guide"]);
  });

  it("shows nothing once the setup was finished or skipped", () => {
    expect(
      onboardingMode({ onboardingCompleted: true }, { hasKey: false }),
    ).toBeNull();
    expect(
      onboardingMode({ onboardingCompleted: true }, { hasKey: true }),
    ).toBeNull();
  });
});

describe("loadStartup", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  function mockCore(
    settings: unknown,
    keyStatus: unknown,
    { keyFails = false } = {},
  ) {
    invokeMock.mockImplementation((command) => {
      if (command === "get_settings") return Promise.resolve(settings);
      if (command === "get_api_key_status")
        return keyFails
          ? // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
            Promise.reject({ code: "credential_store", message: "denied" })
          : Promise.resolve(keyStatus);
      return Promise.reject(new Error(`unexpected ${command}`));
    });
  }

  it("reads the app language and the setup to show", async () => {
    mockCore(
      { uiLanguage: "es", onboardingCompleted: false },
      { hasKey: false, last4: null },
    );

    await expect(loadStartup()).resolves.toEqual({
      uiLanguage: "es",
      onboarding: "full",
    });
  });

  it("skips the setup when it was completed", async () => {
    mockCore(
      { uiLanguage: "en", onboardingCompleted: true },
      { hasKey: true, last4: "abcd" },
    );

    await expect(loadStartup()).resolves.toEqual({
      uiLanguage: "en",
      onboarding: null,
    });
  });

  it("follows the OS and skips the setup without readable settings", async () => {
    invokeMock.mockRejectedValue(new Error("no core"));

    await expect(loadStartup()).resolves.toEqual({
      uiLanguage: "system",
      onboarding: null,
    });
  });

  it("shows the full setup when the key cannot be read", async () => {
    mockCore({ uiLanguage: "en", onboardingCompleted: false }, null, {
      keyFails: true,
    });

    await expect(loadStartup()).resolves.toEqual({
      uiLanguage: "en",
      onboarding: "full",
    });
  });
});

describe("virtual cables", () => {
  const devices = [SPEAKERS, CABLE_OUTPUT, VOICEMEETER, CABLE_INPUT];

  it("lists only the playback side of virtual cables", () => {
    expect(virtualCableOutputs(devices)).toEqual([VOICEMEETER, CABLE_INPUT]);
  });

  it("keeps the cable already chosen", () => {
    expect(preferredCable([VOICEMEETER, CABLE_INPUT], "vm")).toBe(VOICEMEETER);
  });

  it("prefers CABLE Input, then the first cable", () => {
    expect(preferredCable([VOICEMEETER, CABLE_INPUT], null)).toBe(CABLE_INPUT);
    expect(preferredCable([VOICEMEETER, CABLE_INPUT], "speakers")).toBe(
      CABLE_INPUT,
    );
    expect(preferredCable([VOICEMEETER], null)).toBe(VOICEMEETER);
    expect(preferredCable([], null)).toBeNull();
  });
});
