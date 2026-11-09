import { act, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioDevice, Settings } from "../lib/api";
import { render } from "../test/i18n";
import HomeScreen from "./HomeScreen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const invokeMock = vi.mocked(invoke);

const MIC: AudioDevice = {
  id: "mic",
  name: "Microphone (HyperX SoloCast)",
  kind: "input",
  isDefault: true,
  isVirtualCable: false,
};
const CABLE: AudioDevice = {
  id: "cable",
  name: "CABLE Input (VB-Audio Virtual Cable)",
  kind: "output",
  isDefault: false,
  isVirtualCable: true,
};
const SPEAKERS: AudioDevice = {
  id: "speakers",
  name: "Speakers (Realtek(R) Audio)",
  kind: "output",
  isDefault: true,
  isVirtualCable: false,
};

interface Setup {
  hasKey?: boolean;
  fakeTranslator?: boolean;
  outputDeviceId?: string | null;
  devices?: AudioDevice[];
}

function setup({
  hasKey = true,
  fakeTranslator = false,
  outputDeviceId = "cable",
  devices = [MIC, CABLE, SPEAKERS],
}: Setup = {}) {
  const settings: Settings = {
    version: 2,
    uiLanguage: "en",
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
      inputDeviceId: "mic",
      outputDeviceId,
    },
  };
  const answers: Record<string, unknown> = {
    get_settings: settings,
    get_api_key_status: { hasKey, last4: hasKey ? "abcd" : null },
    app_info: {
      name: "LiveDub Voice",
      version: "0.1.0",
      profile: "debug",
      fakeTranslator,
    },
    list_audio_devices: devices,
    get_outgoing_status: { state: "idle", error: null },
  };
  invokeMock.mockImplementation((command) =>
    command in answers
      ? Promise.resolve(answers[command])
      : new Promise(() => {}),
  );
  render(<HomeScreen onOpenSettings={vi.fn()} />);
}

/** Waits until settings, devices and the key check are in. */
async function loaded() {
  await screen.findByText("CABLE Input (VB-Audio Virtual Cable)");
  await act(async () => {});
}

function startButton() {
  return screen.getByRole("button", { name: "Start outgoing" });
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("HomeScreen outgoing route", () => {
  it("can start with a key and a connected cable, showing device names", async () => {
    setup();

    expect(
      await screen.findByText("CABLE Input (VB-Audio Virtual Cable)"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Microphone (HyperX SoloCast)"),
    ).toBeInTheDocument();
    await vi.waitFor(() => expect(startButton()).toBeEnabled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("is blocked without an API key", async () => {
    setup({ hasKey: false });

    expect(
      await screen.findByText("Add your OpenAI API key in Settings to start."),
    ).toBeInTheDocument();
    expect(startButton()).toBeDisabled();
  });

  it("does not need a key with the fake translator", async () => {
    setup({ hasKey: false, fakeTranslator: true });
    await loaded();

    expect(startButton()).toBeEnabled();
    expect(screen.queryByText(/OpenAI API key/)).not.toBeInTheDocument();
  });

  it("is blocked without an output", async () => {
    setup({ outputDeviceId: null });

    expect(
      await screen.findByText(/Choose the virtual cable/),
    ).toBeInTheDocument();
    expect(startButton()).toBeDisabled();
  });

  it("is blocked when the chosen cable is not connected", async () => {
    setup({ devices: [MIC, SPEAKERS] });

    expect(
      await screen.findByText(/The output chosen in Settings is not connected/),
    ).toBeInTheDocument();
    expect(startButton()).toBeDisabled();
  });

  it("warns, but can start, when the output is not a virtual cable", async () => {
    setup({ outputDeviceId: "speakers" });

    expect(await screen.findByRole("status")).toHaveTextContent(
      "not a known virtual cable",
    );
    expect(startButton()).toBeEnabled();
  });
});
