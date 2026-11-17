import { fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioApp, SourceApp } from "./lib/api";
import SourceAppPicker from "./SourceAppPicker";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const chrome: AudioApp = {
  processName: "chrome.exe",
  displayName: "Google Chrome",
  pid: 1100,
  sessionPids: [1101, 1102],
  active: true,
};
const discord: AudioApp = {
  processName: "Discord.exe",
  displayName: "Discord",
  pid: 1200,
  sessionPids: [1201],
  active: false,
};

function mockApps(apps: AudioApp[]) {
  invokeMock.mockImplementation((command) =>
    command === "list_audio_apps"
      ? Promise.resolve(apps)
      : Promise.reject(new Error(`unexpected command ${command}`)),
  );
}

/** Renders the picker as the settings form does: it owns the value. */
function renderPicker(initial: SourceApp, uiLanguage: "en" | "es" = "en") {
  const changes: SourceApp[] = [];
  function Harness() {
    const [value, setValue] = useState(initial);
    return (
      <SourceAppPicker
        value={value}
        onChange={(next) => {
          changes.push(next);
          setValue(next);
        }}
      />
    );
  }
  render(<Harness />, { uiLanguage });
  return changes;
}

function last<T>(items: T[]): T | undefined {
  return items[items.length - 1];
}

function optionLabels(select: HTMLElement): string[] {
  return Array.from(
    (select as HTMLSelectElement).options,
    (option) => option.textContent ?? "",
  );
}

describe("SourceAppPicker", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("translates the whole system by default", async () => {
    mockApps([chrome]);

    renderPicker({ kind: "allExceptSelf" });

    expect(
      await screen.findByRole("radio", { name: "Whole system" }),
    ).toBeChecked();
    expect(
      screen.getByText("Everything playing on this PC except LiveDub Voice."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("App")).not.toBeInTheDocument();
  });

  it("picks the app playing audio when switching to one app", async () => {
    mockApps([chrome, discord]);
    const changes = renderPicker({ kind: "allExceptSelf" });

    fireEvent.click(await screen.findByRole("radio", { name: "One app" }));

    await vi.waitFor(() =>
      expect(last(changes)).toEqual({ kind: "process", name: "chrome.exe" }),
    );
    const select = screen.getByLabelText("App");
    expect(select).toHaveValue("chrome.exe");
    expect(optionLabels(select)).toEqual([
      "Google Chrome · playing",
      "Discord",
    ]);

    fireEvent.change(select, { target: { value: "Discord.exe" } });
    expect(last(changes)).toEqual({ kind: "process", name: "Discord.exe" });

    fireEvent.click(screen.getByRole("radio", { name: "Whole system" }));
    expect(last(changes)).toEqual({ kind: "allExceptSelf" });
  });

  it("keeps a saved app that is not running", async () => {
    mockApps([chrome]);

    renderPicker({ kind: "process", name: "Zoom.exe" });

    const select = await screen.findByLabelText("App");
    await screen.findByRole("option", { name: "Zoom (not running)" });
    expect(select).toHaveValue("Zoom.exe");
    expect(optionLabels(select)).toEqual([
      "Google Chrome · playing",
      "Zoom (not running)",
    ]);
  });

  it("matches the saved app without case", async () => {
    mockApps([discord]);

    renderPicker({ kind: "process", name: "discord.exe" });

    await screen.findByRole("option", { name: "Discord" });
    expect(screen.getByLabelText("App")).toHaveValue("Discord.exe");
    expect(
      screen.queryByRole("option", { name: /not running/ }),
    ).not.toBeInTheDocument();
  });

  it("explains what happens while no app is playing or chosen", async () => {
    mockApps([]);
    const changes = renderPicker({ kind: "allExceptSelf" });

    fireEvent.click(await screen.findByRole("radio", { name: "One app" }));

    expect(
      await screen.findByText(/No app is playing audio right now/),
    ).toBeInTheDocument();
    expect(changes).toEqual([]);
    expect(
      screen.getByText(
        "Until you choose an app, the whole system is translated.",
      ),
    ).toBeInTheDocument();
    expect(optionLabels(screen.getByLabelText("App"))).toEqual([
      "Choose an app…",
    ]);
  });

  it("refreshes the app list", async () => {
    mockApps([]);
    renderPicker({ kind: "process", name: "Discord.exe" });
    await screen.findByRole("option", { name: "Discord (not running)" });

    mockApps([discord]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh apps" }));

    expect(
      await screen.findByRole("option", { name: "Discord" }),
    ).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("shows why the apps could not be listed", async () => {
    invokeMock.mockRejectedValue({
      code: "audio",
      message: "audio device error: could not list the apps playing audio",
    });

    renderPicker({ kind: "process", name: "Discord.exe" });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not list the apps playing audio: Audio device error",
    );
  });

  it("speaks Spanish", async () => {
    mockApps([chrome]);

    renderPicker({ kind: "process", name: "Zoom.exe" }, "es");

    expect(await screen.findByRole("radio", { name: "Una app" })).toBeChecked();
    expect(screen.getByText("Traducir el audio de")).toBeInTheDocument();
    await screen.findByRole("option", { name: "Zoom (no está abierta)" });
    expect(
      screen.getByRole("option", { name: "Google Chrome · sonando" }),
    ).toBeInTheDocument();
  });
});
