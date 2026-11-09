import { fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SettingsForm from "./SettingsForm";
import type { Settings } from "./lib/api";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const defaults: Settings = {
  version: 1,
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

describe("SettingsForm", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("loads the settings and saves the edited values", async () => {
    invokeMock.mockImplementation((command, args) =>
      command === "get_settings"
        ? Promise.resolve(defaults)
        : Promise.resolve((args as { settings: Settings }).settings),
    );

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
    fireEvent.change(screen.getByLabelText("Virtual cable device id"), {
      target: { value: "cable-input" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "Settings saved.",
    );
    expect(invokeMock).toHaveBeenCalledWith("save_settings", {
      settings: {
        version: 1,
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
      "Could not save settings: invalid settings: bad volume",
    );
  });

  it("shows an error when the settings cannot be loaded", async () => {
    invokeMock.mockRejectedValue({ code: "io", message: "I/O error: denied" });

    render(<SettingsForm />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load settings: I/O error: denied",
    );
  });
});
