import { act, fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SettingsForm from "./SettingsForm";
import type { AudioApp, AudioDevice, HotkeyProblem, Settings } from "./lib/api";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const defaults: Settings = {
  version: 6,
  uiLanguage: "system",
  incoming: {
    enabled: false,
    targetLanguage: "es",
    sourceApp: { kind: "allExceptSelf" },
    outputDeviceId: null,
    originalVolume: 20,
  },
  outgoing: {
    enabled: false,
    targetLanguage: "en",
    inputDeviceId: null,
    outputDeviceId: null,
    talkMode: "voiceDetection",
  },
  spendingCap: { dailyUsd: null, monthlyUsd: null },
  voiceActivity: { pauseWhenSilent: true, autoOffMinutes: 5 },
  hotkeys: { bindings: {}, soundCues: true },
  closeQuitsApp: false,
  trayNoticeShown: false,
  onboardingCompleted: true,
};

const speakers: AudioDevice = {
  id: "spk",
  name: "Speakers (Realtek(R) Audio)",
  kind: "output",
  isDefault: true,
  isVirtualCable: false,
};
const cableInput: AudioDevice = {
  id: "cable-input",
  name: "CABLE Input (VB-Audio Virtual Cable)",
  kind: "output",
  isDefault: false,
  isVirtualCable: true,
};
const microphone: AudioDevice = {
  id: "mic",
  name: "Microphone (USB)",
  kind: "input",
  isDefault: true,
  isVirtualCable: false,
};

const chrome: AudioApp = {
  processName: "chrome.exe",
  displayName: "Google Chrome",
  pid: 1100,
  sessionPids: [1102],
  active: true,
};
const discord: AudioApp = {
  processName: "Discord.exe",
  displayName: "Discord",
  pid: 1200,
  sessionPids: [1201],
  active: false,
};

/**
 * Mocks the core: `get_settings` returns `settings`, `save_settings` echoes what it gets, and
 * the translations are in `states` (idle by default).
 */
function mockCore(
  settings: Settings,
  devices: AudioDevice[],
  apps: AudioApp[] = [chrome, discord],
  states: { incoming?: string; outgoing?: string } = {},
  hotkeyProblems: HotkeyProblem[] = [],
) {
  invokeMock.mockImplementation((command, args) => {
    switch (command) {
      case "get_incoming_status":
        return Promise.resolve({
          state: states.incoming ?? "idle",
          error: null,
        });
      case "get_outgoing_status":
        return Promise.resolve({
          state: states.outgoing ?? "idle",
          error: null,
        });
      case "get_settings":
        return Promise.resolve(settings);
      case "list_audio_devices":
        return Promise.resolve(devices);
      case "list_audio_apps":
        return Promise.resolve(apps);
      case "get_hotkey_status":
        return Promise.resolve({ problems: hotkeyProblems, suspended: false });
      default:
        return Promise.resolve(
          (args as { settings?: Settings } | undefined)?.settings,
        );
    }
  });
}

/** Settings sent in the last `save_settings` call. */
function savedSettings(): Settings {
  const call = invokeMock.mock.calls
    .filter(([command]) => command === "save_settings")
    .pop();
  return (call?.[1] as { settings: Settings }).settings;
}

function optionLabels(select: HTMLElement): string[] {
  return Array.from(
    (select as HTMLSelectElement).options,
    (option) => option.textContent ?? "",
  );
}

describe("SettingsForm", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("says that changes apply on the next start while a direction runs", async () => {
    mockCore(defaults, [speakers], [chrome, discord], { outgoing: "live" });

    render(<SettingsForm />);

    expect(
      await screen.findByText(
        "The outgoing translation is running. Changes here apply the next time you start it.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/The incoming translation is running/),
    ).not.toBeInTheDocument();
  });

  it("offers every target language, localized, with English and Spanish on top", async () => {
    mockCore(defaults, [speakers]);

    render(<SettingsForm />);

    const picker = await screen.findByLabelText("Translate others into");
    const labels = optionLabels(picker);
    expect(labels).toHaveLength(13);
    expect(labels.slice(0, 3)).toEqual(["English", "Spanish", "Chinese"]);
    expect(labels).toContain("Japanese");
    expect(
      screen.getByRole("img", { name: "Any language to Spanish" }),
    ).toHaveTextContent(/^Auto.*ES$/);

    fireEvent.change(screen.getByLabelText("Translate you into"), {
      target: { value: "fr" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByRole("status");
    expect(savedSettings().outgoing.targetLanguage).toBe("fr");
  });

  it("names the target languages in Spanish when the panel is in Spanish", async () => {
    mockCore(defaults, [speakers]);

    render(<SettingsForm />, { uiLanguage: "es" });

    const picker = await screen.findByLabelText(
      "Idioma para escuchar a los demás",
    );
    const labels = optionLabels(picker);
    expect(labels.slice(0, 2)).toEqual(["Español", "Inglés"]);
    expect(labels).toContain("Alemán");
    expect(
      screen.getByRole("img", { name: "De cualquier idioma a español" }),
    ).toHaveTextContent(/^Cualquiera.*ES$/);
  });

  it("loads the settings and saves the edited values", async () => {
    mockCore(defaults, [speakers, cableInput, microphone]);

    render(<SettingsForm />);

    fireEvent.click(await screen.findByLabelText("Translate incoming audio"));
    fireEvent.change(screen.getByLabelText(/Original audio volume/), {
      target: { value: "35" },
    });
    fireEvent.click(screen.getByRole("radio", { name: "One app" }));
    await screen.findByRole("option", { name: "Discord" });
    fireEvent.change(screen.getByLabelText("App"), {
      target: { value: "Discord.exe" },
    });
    fireEvent.change(screen.getByLabelText("Translate you into"), {
      target: { value: "es" },
    });
    const cable = screen.getByLabelText("Virtual cable");
    await screen.findAllByRole("option", { name: cableInput.name });
    fireEvent.change(cable, { target: { value: "cable-input" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "Settings saved.",
    );
    expect(invokeMock).toHaveBeenCalledWith("save_settings", {
      settings: {
        version: 6,
        // The form saves the language the panel is showing (English here).
        uiLanguage: "en",
        incoming: {
          enabled: true,
          targetLanguage: "es",
          sourceApp: { kind: "process", name: "Discord.exe" },
          outputDeviceId: null,
          originalVolume: 35,
        },
        outgoing: {
          enabled: false,
          targetLanguage: "es",
          inputDeviceId: null,
          outputDeviceId: "cable-input",
          talkMode: "voiceDetection",
        },
        spendingCap: { dailyUsd: null, monthlyUsd: null },
        voiceActivity: { pauseWhenSilent: true, autoOffMinutes: 5 },
        hotkeys: { bindings: {}, soundCues: true },
        closeQuitsApp: false,
        trayNoticeShown: false,
        onboardingCompleted: true,
      },
    });
  });

  it("shows the error returned by save_settings", async () => {
    invokeMock.mockImplementation((command) =>
      command === "get_settings"
        ? Promise.resolve(defaults)
        : command === "list_audio_devices"
          ? Promise.resolve([])
          : // Tauri rejects failing commands with a plain `{ code, message }` object.
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
            Promise.reject({
              code: "invalid_settings",
              message: "invalid settings: bad volume",
            }),
    );

    render(<SettingsForm />);
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save settings: The settings are not valid (bad volume).",
    );
  });

  it("shows an error when the settings cannot be loaded", async () => {
    invokeMock.mockRejectedValue({ code: "io", message: "I/O error: denied" });

    render(<SettingsForm />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load settings: A file could not be read or written (denied).",
    );
  });

  it("offers the system default and the devices of each kind", async () => {
    mockCore(defaults, [speakers, cableInput, microphone]);

    render(<SettingsForm />);
    await screen.findAllByRole("option", {
      name: "Microphone (USB) (default)",
    });

    const headphones = screen.getByLabelText("Headphones");
    expect(headphones).toHaveValue("");
    expect(optionLabels(headphones)).toEqual([
      "System default",
      "Speakers (Realtek(R) Audio) (default)",
      "CABLE Input (VB-Audio Virtual Cable)",
    ]);
    expect(optionLabels(screen.getByLabelText("Microphone"))).toEqual([
      "System default",
      "Microphone (USB) (default)",
    ]);
  });

  it("lists virtual cables first for the outgoing output", async () => {
    mockCore(defaults, [speakers, cableInput, microphone]);

    render(<SettingsForm />);
    await screen.findAllByRole("option", { name: cableInput.name });

    expect(optionLabels(screen.getByLabelText("Virtual cable"))).toEqual([
      "System default",
      "CABLE Input (VB-Audio Virtual Cable)",
      "Speakers (Realtek(R) Audio) (default)",
    ]);
    expect(
      screen.queryByText(/No virtual cable found/),
    ).not.toBeInTheDocument();
  });

  it("keeps a saved device that is no longer available", async () => {
    mockCore(
      {
        ...defaults,
        incoming: { ...defaults.incoming, outputDeviceId: "gone" },
      },
      [speakers, microphone],
    );

    render(<SettingsForm />);
    await screen.findByRole("option", { name: "Unavailable device" });

    const headphones = screen.getByLabelText("Headphones");
    expect(headphones).toHaveValue("gone");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("Settings saved.");
    expect(savedSettings().incoming.outputDeviceId).toBe("gone");
  });

  it("stores the system default as null", async () => {
    mockCore(
      {
        ...defaults,
        outgoing: { ...defaults.outgoing, inputDeviceId: "mic" },
      },
      [speakers, microphone],
    );

    render(<SettingsForm />);
    await screen.findAllByRole("option", {
      name: "Microphone (USB) (default)",
    });
    fireEvent.change(screen.getByLabelText("Microphone"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("Settings saved.");
    expect(savedSettings().outgoing.inputDeviceId).toBeNull();
  });

  it("hints to install a virtual cable when none is found", async () => {
    mockCore(defaults, [speakers, microphone]);

    render(<SettingsForm />);

    expect(
      await screen.findByText(/Install a virtual cable such as VB-CABLE/),
    ).toBeInTheDocument();
  });

  it("refreshes the device list", async () => {
    mockCore(defaults, [speakers, microphone]);
    render(<SettingsForm />);
    await screen.findByText(/No virtual cable found/);

    mockCore(defaults, [speakers, cableInput, microphone]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh devices" }));

    await screen.findAllByRole("option", { name: cableInput.name });
    expect(
      screen.queryByText(/No virtual cable found/),
    ).not.toBeInTheDocument();
  });

  it("shows an error when the devices cannot be listed", async () => {
    invokeMock.mockImplementation((command) =>
      command === "get_settings"
        ? Promise.resolve(defaults)
        : // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({
            code: "audio",
            message: "audio device error: could not list audio devices",
          }),
    );

    render(<SettingsForm />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not list audio devices: Audio device error (could not list audio devices).",
    );
    expect(screen.getByLabelText("Headphones")).toHaveValue("");
  });

  it("saves the voice detection and window options", async () => {
    mockCore(defaults, [speakers, cableInput, microphone]);
    render(<SettingsForm />);

    const pause = await screen.findByRole("switch", {
      name: "Don't send silence",
    });
    expect(pause).toBeChecked();
    const autoOff = screen.getByLabelText("Turn off when nobody speaks");
    expect(autoOff).toHaveValue("5");
    expect(optionLabels(autoOff)).toEqual([
      "Never",
      "After 1 minute",
      "After 2 minutes",
      "After 5 minutes",
      "After 10 minutes",
      "After 15 minutes",
      "After 30 minutes",
      "After 60 minutes",
    ]);
    const closeQuits = screen.getByRole("switch", {
      name: "Closing the window quits the app",
    });
    expect(closeQuits).not.toBeChecked();

    fireEvent.click(pause);
    fireEvent.change(autoOff, { target: { value: "never" } });
    fireEvent.click(closeQuits);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("Settings saved.");
    const saved = savedSettings();
    expect(saved.voiceActivity).toEqual({
      pauseWhenSilent: false,
      autoOffMinutes: null,
    });
    expect(saved.closeQuitsApp).toBe(true);
  });

  it("keeps an auto-off time that is not among the choices", async () => {
    mockCore(
      {
        ...defaults,
        voiceActivity: { pauseWhenSilent: true, autoOffMinutes: 7 },
      },
      [speakers],
    );
    render(<SettingsForm />);

    const autoOff = await screen.findByLabelText("Turn off when nobody speaks");
    expect(autoOff).toHaveValue("7");
    expect(optionLabels(autoOff)).toContain("After 7 minutes");
  });

  it("switches outgoing to push-to-talk with its key and saves it", async () => {
    mockCore(defaults, [speakers]);
    render(<SettingsForm />);

    const pushToTalk = await screen.findByRole("radio", {
      name: "Push-to-talk",
    });
    expect(screen.getByRole("radio", { name: "When you speak" })).toBeChecked();
    expect(
      screen.queryByRole("button", { name: "Push-to-talk key" }),
    ).not.toBeInTheDocument();

    fireEvent.click(pushToTalk);

    const key = screen.getByRole("button", { name: "Push-to-talk key" });
    expect(key).toHaveTextContent("F8");
    expect(screen.getByText(/Only while you hold the key/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Settings saved.");
    expect(savedSettings().outgoing.talkMode).toBe("pushToTalk");
  });

  it("asks for a key when push-to-talk has none", async () => {
    mockCore(
      {
        ...defaults,
        outgoing: { ...defaults.outgoing, talkMode: "pushToTalk" },
        hotkeys: { bindings: { pushToTalk: null }, soundCues: true },
      },
      [speakers],
    );
    render(<SettingsForm />);

    expect(
      await screen.findByText("Choose a key to use push-to-talk."),
    ).toBeInTheDocument();
  });

  it("records, clears and resets the toggle shortcuts", async () => {
    mockCore(defaults, [speakers]);
    render(<SettingsForm />);

    const both = await screen.findByRole("button", {
      name: "Turn both on or off",
    });
    expect(both).toHaveTextContent("Ctrl + Alt + B");

    // Another action's shortcut is refused.
    fireEvent.click(both);
    fireEvent.keyDown(both, {
      code: "KeyI",
      key: "i",
      ctrlKey: true,
      altKey: true,
    });
    await act(async () => {});
    expect(
      screen.getByText(
        "Ctrl + Alt + I is already used for “Turn incoming on or off”.",
      ),
    ).toBeInTheDocument();
    expect(both).toHaveTextContent("Ctrl + Alt + B");

    // A free one is taken.
    fireEvent.click(both);
    fireEvent.keyDown(both, { code: "F10", key: "F10" });
    await act(async () => {});
    expect(both).toHaveTextContent("F10");

    // Clearing the incoming one.
    const incoming = screen.getByRole("button", {
      name: "Turn incoming on or off",
    });
    const clear = incoming.parentElement?.querySelector(
      "button[aria-label='Clear the shortcut']",
    );
    fireEvent.click(clear as Element);
    expect(incoming).toHaveTextContent("None");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Settings saved.");
    expect(savedSettings().hotkeys.bindings).toMatchObject({
      toggleIncoming: null,
      toggleBoth: "F10",
    });

    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect(incoming).toHaveTextContent("Ctrl + Alt + I");
    expect(both).toHaveTextContent("Ctrl + Alt + B");
  });

  it("says which shortcuts Windows refused", async () => {
    mockCore(defaults, [speakers], [chrome], {}, [
      {
        action: "toggleOutgoing",
        shortcut: "Ctrl+Alt+KeyO",
        reason: "typesCharacter",
      },
      {
        action: "toggleBoth",
        shortcut: "Ctrl+Alt+KeyB",
        reason: "unavailable",
      },
    ]);
    render(<SettingsForm />);

    expect(
      await screen.findByText(
        "Ctrl + Alt + O types a character on your keyboard layout (AltGr), so it is not used. Choose another one.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Ctrl + Alt + B is not working: another app is using it, or Windows reserves it. Choose another one.",
      ),
    ).toBeInTheDocument();
  });

  it("explains a save refused because Windows refused a shortcut", async () => {
    mockCore(defaults, [speakers]);
    const core = invokeMock.getMockImplementation();
    invokeMock.mockImplementation((command, args) =>
      command === "save_settings"
        ? // Tauri rejects failing commands with a plain `{ code, message }` object.
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({
            code: "hotkey_unavailable",
            message:
              "shortcut not available (another app may be using it): Ctrl+Alt+KeyB",
          })
        : (core?.(command, args) as Promise<unknown>),
    );
    render(<SettingsForm />);

    fireEvent.click(await screen.findByRole("button", { name: "Save" }));

    expect(
      await screen.findByText(
        "Could not save settings: Windows refused the shortcut Ctrl + Alt + B: another app may be using it. Choose another one.",
      ),
    ).toBeInTheDocument();
  });

  it("turns the sound cues off", async () => {
    mockCore(defaults, [speakers]);
    render(<SettingsForm />);

    const cues = await screen.findByRole("switch", { name: "Sound cues" });
    expect(cues).toBeChecked();
    fireEvent.click(cues);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("Settings saved.");
    expect(savedSettings().hotkeys.soundCues).toBe(false);
  });

  it("quits the app from Settings", async () => {
    mockCore(defaults, [speakers]);
    render(<SettingsForm />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Quit LiveDub Voice" }),
    );

    expect(invokeMock).toHaveBeenCalledWith("quit_app");
  });
});
