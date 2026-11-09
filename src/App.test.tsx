import { fireEvent, render, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

/** Resolves `app_info`; every other command stays pending. */
function mockAppInfo() {
  invokeMock.mockImplementation((command) =>
    command === "app_info"
      ? Promise.resolve({
          name: "LiveDub Voice",
          version: "1.2.3",
          profile: "debug",
        })
      : new Promise(() => {}),
  );
}

function navItem(name: string) {
  return within(screen.getByRole("navigation", { name: "Main" })).getByRole(
    "button",
    { name },
  );
}

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("renders the product name and the version from app_info", async () => {
    mockAppInfo();

    render(<App />);

    expect(screen.getByText("LiveDub Voice")).toBeInTheDocument();
    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("app_info");
  });

  it("shows the error message when app_info fails", async () => {
    invokeMock.mockImplementation((command) =>
      command === "app_info"
        ? // Tauri rejects failing commands with a plain `{ code, message }` object.
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({ code: "internal", message: "boom" })
        : new Promise(() => {}),
    );

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read app info: boom",
    );
  });

  it("starts on Home with the Home nav item marked as current", () => {
    mockAppInfo();

    render(<App />);

    expect(
      screen.getByRole("heading", { level: 1, name: "Home" }),
    ).toBeInTheDocument();
    expect(navItem("Home")).toHaveAttribute("aria-current", "page");
    expect(navItem("Settings")).not.toHaveAttribute("aria-current");
    expect(navItem("Usage")).not.toHaveAttribute("aria-current");
  });

  it("switches screens from the sidebar and moves focus to the new title", () => {
    mockAppInfo();

    render(<App />);

    fireEvent.click(navItem("Usage"));
    const usageTitle = screen.getByRole("heading", { level: 1, name: "Usage" });
    expect(usageTitle).toHaveFocus();
    expect(screen.getByText("No usage yet")).toBeInTheDocument();
    expect(navItem("Usage")).toHaveAttribute("aria-current", "page");
    expect(navItem("Home")).not.toHaveAttribute("aria-current");

    fireEvent.click(navItem("Settings"));
    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toHaveFocus();
    expect(navItem("Settings")).toHaveAttribute("aria-current", "page");
    expect(invokeMock).toHaveBeenCalledWith("get_settings");
  });

  it("opens Settings from the Home screen", () => {
    mockAppInfo();

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));

    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toBeInTheDocument();
    expect(navItem("Settings")).toHaveAttribute("aria-current", "page");
  });

  it("shows both routes on Home with the saved languages", async () => {
    invokeMock.mockImplementation((command) => {
      if (command === "get_settings") {
        return Promise.resolve({
          version: 1,
          incoming: {
            enabled: false,
            targetLanguage: "en",
            sourceApp: { kind: "process", name: "Discord.exe" },
            outputDeviceId: null,
            originalVolume: 20,
          },
          outgoing: {
            enabled: false,
            targetLanguage: "es",
            inputDeviceId: null,
            outputDeviceId: null,
          },
        });
      }
      return new Promise(() => {});
    });

    render(<App />);

    expect(await screen.findByText("Discord.exe")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Spanish to English" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "English to Spanish" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Start incoming" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeDisabled();
  });
});
