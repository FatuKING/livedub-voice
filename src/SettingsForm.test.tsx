import { fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SettingsForm from "./SettingsForm";
import type { AudioDevice, Settings } from "./lib/api";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const defaults: Settings = {
  version: 2,
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
  },
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

/** Mocks the core: `get_settings` returns `settings`, `save_settings` echoes what it gets. */
function mockCore(settings: Settings, devices: AudioDevice[]) {
  invokeMock.mockImplementation((command, args) => {
    switch (command) {
      case "get_settings":
        return Promise.resolve(settings);
      case "list_audio_devices":
        return Promise.resolve(devices);
      default:
        return Promise.resolve((args as { settings: Settings }).settings);
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

  it("loads the settings and saves the edited values", async () => {
    mockCore(defaults, [speakers, cableInput, microphone]);

    render(<SettingsForm />);

    fireEvent.click(await screen.findByLabelText("Translate incoming audio"));
    fireEvent.change(screen.getByLabelText(/Original audio volume/), {
      target: { value: "35" },
    });
    fireEvent.change(screen.getByLabelText(/Source app/), {
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
        version: 2,
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
        },
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
});
