import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ApiKeyStatus,
  AudioDevice,
  KeyValidation,
  Settings,
} from "../lib/api";
import type { OnboardingMode } from "../lib/onboarding";
import { render } from "../test/i18n";
import Onboarding from "./Onboarding";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

const invokeMock = vi.mocked(invoke);
const openUrlMock = vi.mocked(openUrl);

/** Fake key: never a real one. */
const FAKE_KEY = "sk-test-0000000000000000000000abcd";

const SETTINGS: Settings = {
  version: 5,
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
    inputDeviceId: null,
    outputDeviceId: null,
  },
  spendingCap: { dailyUsd: null, monthlyUsd: null },
  voiceActivity: { pauseWhenSilent: true, autoOffMinutes: 5 },
  closeQuitsApp: false,
  trayNoticeShown: false,
  onboardingCompleted: false,
};

const MIC: AudioDevice = {
  id: "mic",
  name: "Microphone (HyperX SoloCast)",
  kind: "input",
  isDefault: true,
  isVirtualCable: false,
};
const SPEAKERS: AudioDevice = {
  id: "speakers",
  name: "Speakers (Realtek(R) Audio)",
  kind: "output",
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

interface Backend {
  settings?: Settings;
  /** Devices for every listing, or one list per call. */
  devices?: AudioDevice[] | AudioDevice[][];
  validation?: KeyValidation;
  keyStatus?: ApiKeyStatus;
}

/** Mocks the core; `save_settings` keeps what it gets, like the real store. */
function mockBackend({
  settings = SETTINGS,
  devices = [MIC, SPEAKERS],
  validation = { kind: "valid" },
  keyStatus = { hasKey: false, last4: null },
}: Backend = {}) {
  let current = settings;
  const lists = Array.isArray(devices[0])
    ? [...(devices as AudioDevice[][])]
    : null;
  invokeMock.mockImplementation((command, args) => {
    switch (command) {
      case "get_settings":
        return Promise.resolve(current);
      case "save_settings":
        current = (args as { settings: Settings }).settings;
        return Promise.resolve(current);
      case "complete_onboarding":
        current = { ...current, onboardingCompleted: true };
        return Promise.resolve(current);
      case "list_audio_devices":
        return Promise.resolve(
          lists ? (lists.length > 1 ? lists.shift() : lists[0]) : devices,
        );
      case "get_api_key_status":
        return Promise.resolve(keyStatus);
      case "save_api_key":
        return Promise.resolve({
          status: { hasKey: true, last4: "abcd" },
          validation,
        });
      default:
        return new Promise(() => {});
    }
  });
}

function renderOnboarding(mode: OnboardingMode = "full") {
  const onDone = vi.fn();
  render(<Onboarding mode={mode} onDone={onDone} />);
  return onDone;
}

function stepper() {
  return screen.getByRole("navigation", { name: "Setup steps" });
}

function next() {
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
}

/** Jumps to a step through the step indicator. */
function goTo(step: string) {
  fireEvent.click(within(stepper()).getByRole("button", { name: step }));
}

describe("Onboarding", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    openUrlMock.mockClear();
  });

  it("starts on the welcome step with a step indicator", () => {
    mockBackend();

    renderOnboarding();

    expect(
      screen.getByRole("heading", {
        level: 1,
        name: "Welcome to LiveDub Voice",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Step 1 of 6")).toBeInTheDocument();
    const steps = within(stepper()).getAllByRole("button");
    expect(steps.map((step) => step.textContent)).toEqual([
      "1Welcome",
      "2OpenAI API key",
      "3Headphones",
      "4Virtual cable",
      "5Privacy",
      "6Quick guide",
    ]);
    expect(steps[0]).toHaveAttribute("aria-current", "step");
    expect(
      screen.queryByRole("button", { name: "Back" }),
    ).not.toBeInTheDocument();
  });

  it("moves between steps with Next and Back, focusing each title", () => {
    mockBackend();
    renderOnboarding();

    next();

    const title = screen.getByRole("heading", {
      level: 1,
      name: "Connect your OpenAI account",
    });
    expect(title).toHaveFocus();
    expect(screen.getByText("Step 2 of 6")).toBeInTheDocument();
    expect(
      within(stepper()).getByRole("button", { name: "Welcome (done)" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(
      screen.getByRole("heading", {
        level: 1,
        name: "Welcome to LiveDub Voice",
      }),
    ).toHaveFocus();
  });

  it("applies and saves the app language from the welcome step", async () => {
    mockBackend();
    renderOnboarding();

    fireEvent.change(screen.getByLabelText("App language"), {
      target: { value: "es" },
    });

    expect(
      await screen.findByRole("heading", {
        level: 1,
        name: "Bienvenida a LiveDub Voice",
      }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("save_settings", {
        settings: { ...SETTINGS, uiLanguage: "es" },
      }),
    );
  });

  it("skips the whole setup and remembers it", async () => {
    mockBackend();
    const onDone = renderOnboarding();

    fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(invokeMock).toHaveBeenCalledWith("complete_onboarding");
  });

  it("finishes on the last step and remembers it", async () => {
    mockBackend();
    const onDone = renderOnboarding();

    goTo("Quick guide");
    expect(
      screen.getByRole("heading", {
        level: 1,
        name: "How to use LiveDub Voice",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Discord/ })).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Start using LiveDub Voice" }),
    );

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(invokeMock).toHaveBeenCalledWith("complete_onboarding");
  });

  it("still leaves the setup when it cannot be remembered", async () => {
    mockBackend();
    invokeMock.mockImplementation((command) =>
      command === "complete_onboarding"
        ? // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({ code: "io", message: "denied" })
        : new Promise(() => {}),
    );
    const onDone = renderOnboarding();

    fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
  });

  describe("API key step", () => {
    function enterKey() {
      goTo("OpenAI API key");
      fireEvent.change(screen.getByLabelText("API key"), {
        target: { value: FAKE_KEY },
      });
      fireEvent.click(
        screen.getByRole("button", { name: "Save and validate" }),
      );
    }

    it("explains the cost per direction and links to OpenAI", () => {
      mockBackend();
      renderOnboarding();
      goTo("OpenAI API key");

      expect(
        screen.getByText(
          /\$0\.034 per audio minute for each direction that is on: about \$2 per hour with one direction/,
        ),
      ).toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", {
          name: "Open platform.openai.com/api-keys",
        }),
      );
      expect(openUrlMock).toHaveBeenCalledWith(
        "https://platform.openai.com/api-keys",
      );
      fireEvent.click(
        screen.getByRole("button", { name: "Set a budget in OpenAI" }),
      );
      expect(openUrlMock).toHaveBeenCalledWith(
        "https://platform.openai.com/settings/organization/limits",
      );
    });

    it("saves and validates a key OpenAI accepts", async () => {
      mockBackend({ validation: { kind: "valid" } });
      renderOnboarding();

      enterKey();

      expect(
        await screen.findByText(/OpenAI accepted the key/),
      ).toBeInTheDocument();
      expect(invokeMock).toHaveBeenCalledWith("save_api_key", {
        key: FAKE_KEY,
      });
      expect(screen.getByText("Valid")).toBeInTheDocument();
    });

    it("reports a key OpenAI rejects", async () => {
      mockBackend({ validation: { kind: "invalidKey" } });
      renderOnboarding();

      enterKey();

      expect(await screen.findByRole("alert")).toHaveTextContent(
        /OpenAI rejected this key/,
      );
      expect(screen.getByText("Invalid")).toBeInTheDocument();
    });
  });

  describe("virtual cable step", () => {
    it("shows a cable as ready and chooses it as the outgoing output", async () => {
      mockBackend({ devices: [MIC, SPEAKERS, CABLE] });
      renderOnboarding();

      goTo("Virtual cable");

      expect(
        await screen.findByText(/Chosen as the outgoing output/),
      ).toBeInTheDocument();
      expect(screen.getByText("Virtual cable found")).toBeInTheDocument();
      expect(screen.getByText("Ready")).toBeInTheDocument();
      expect(
        screen.getByText(/sends your translated voice to CABLE Input/),
      ).toBeInTheDocument();
      expect(invokeMock).toHaveBeenCalledWith("save_settings", {
        settings: {
          ...SETTINGS,
          outgoing: { ...SETTINGS.outgoing, outputDeviceId: "cable" },
        },
      });
    });

    it("keeps a cable that is already chosen without saving", async () => {
      mockBackend({
        settings: {
          ...SETTINGS,
          outgoing: { ...SETTINGS.outgoing, outputDeviceId: "cable" },
        },
        devices: [MIC, SPEAKERS, CABLE],
      });
      renderOnboarding();

      goTo("Virtual cable");

      expect(
        await screen.findByText(/Chosen as the outgoing output/),
      ).toBeInTheDocument();
      expect(invokeMock).not.toHaveBeenCalledWith(
        "save_settings",
        expect.anything(),
      );
    });

    it("explains how to get a cable when none is installed", async () => {
      mockBackend({
        devices: [
          [MIC, SPEAKERS],
          [MIC, SPEAKERS, CABLE],
        ],
      });
      renderOnboarding();

      goTo("Virtual cable");

      expect(
        await screen.findByText("No virtual cable found"),
      ).toBeInTheDocument();
      expect(screen.getByText("Not installed")).toBeInTheDocument();
      expect(
        screen.getByText(/the incoming translation does not need a cable/),
      ).toBeInTheDocument();
      expect(invokeMock).not.toHaveBeenCalledWith(
        "save_settings",
        expect.anything(),
      );

      fireEvent.click(screen.getByRole("button", { name: "Get VB-CABLE" }));
      expect(openUrlMock).toHaveBeenCalledWith("https://vb-audio.com/Cable/");

      // Installed meanwhile: looking again finds it.
      fireEvent.click(screen.getByRole("button", { name: "Check again" }));
      expect(
        await screen.findByText("Virtual cable found"),
      ).toBeInTheDocument();
    });

    it("can be skipped without a cable", async () => {
      mockBackend();
      renderOnboarding();
      goTo("Virtual cable");
      await screen.findByText("No virtual cable found");

      next();

      expect(
        screen.getByRole("heading", { level: 1, name: "Privacy and consent" }),
      ).toBeInTheDocument();
    });
  });

  it("tells existing users only about privacy and the guide (light setup)", async () => {
    mockBackend();
    const onDone = renderOnboarding("light");

    expect(
      screen.getByRole("heading", { level: 1, name: "Privacy and consent" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Your setup is already done/)).toBeInTheDocument();
    expect(screen.getByText("Step 1 of 2")).toBeInTheDocument();
    expect(
      within(stepper())
        .getAllByRole("button")
        .map((step) => step.textContent),
    ).toEqual(["1Privacy", "2Quick guide"]);
    expect(
      screen.getByText(
        /sends the voices of the people you talk with to OpenAI/,
      ),
    ).toBeInTheDocument();

    next();
    fireEvent.click(
      screen.getByRole("button", { name: "Start using LiveDub Voice" }),
    );

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(invokeMock).toHaveBeenCalledWith("complete_onboarding");
  });
});
