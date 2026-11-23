import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
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
      "Could not read app info: Something went wrong inside LiveDub Voice (boom).",
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
    expect(
      screen.getByText(/This is not your OpenAI bill/),
    ).toBeInTheDocument();
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
          version: 5,
          uiLanguage: "system",
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
          spendingCap: { dailyUsd: null, monthlyUsd: null },
          voiceActivity: { pauseWhenSilent: true, autoOffMinutes: 5 },
          closeQuitsApp: false,
          trayNoticeShown: false,
          onboardingCompleted: true,
        });
      }
      return new Promise(() => {});
    });

    render(<App />);

    expect(await screen.findByText("Discord.exe")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Any language to English" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Any language to Spanish" }),
    ).toBeInTheDocument();
    // Incoming plays on the default headphones; outgoing needs a cable first.
    expect(
      screen.getByRole("button", { name: "Start incoming" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeDisabled();
  });
  describe("first-run setup", () => {
    it("shows the setup instead of the panel when asked at launch", () => {
      mockAppInfo();

      render(<App onboarding="full" />);

      expect(
        screen.getByRole("heading", {
          level: 1,
          name: "Welcome to LiveDub Voice",
        }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("navigation", { name: "Main" }),
      ).not.toBeInTheDocument();
    });

    it("lands on Home once the setup is skipped", async () => {
      invokeMock.mockImplementation((command) => {
        if (command === "app_info")
          return Promise.resolve({ name: "LiveDub Voice", version: "1.2.3" });
        if (command === "complete_onboarding") return Promise.resolve({});
        return new Promise(() => {});
      });
      render(<App onboarding="full" />);

      fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));

      const home = await screen.findByRole("heading", {
        level: 1,
        name: "Home",
      });
      await waitFor(() => expect(home).toHaveFocus());
      expect(invokeMock).toHaveBeenCalledWith("complete_onboarding");
      expect(navItem("Home")).toHaveAttribute("aria-current", "page");
    });

    it("runs the full setup again from Settings", () => {
      mockAppInfo();
      render(<App />);

      fireEvent.click(navItem("Settings"));
      fireEvent.click(
        within(
          screen.getByRole("region", { name: "First-run setup" }),
        ).getByRole("button", { name: "Run setup again" }),
      );

      expect(
        screen.getByRole("heading", {
          level: 1,
          name: "Welcome to LiveDub Voice",
        }),
      ).toBeInTheDocument();
      expect(screen.getByText("Step 1 of 6")).toBeInTheDocument();
    });

    it("opens the quick guide from Home", () => {
      mockAppInfo();
      render(<App />);

      const toggle = screen.getByRole("button", { name: "How to use" });
      expect(toggle).toHaveAttribute("aria-expanded", "false");
      expect(
        screen.queryByRole("region", { name: "How to use LiveDub Voice" }),
      ).not.toBeInTheDocument();

      fireEvent.click(toggle);

      const guide = screen.getByRole("region", {
        name: "How to use LiveDub Voice",
      });
      expect(
        within(guide).getByRole("row", { name: /Google Meet/ }),
      ).toBeInTheDocument();
      const hide = screen.getByRole("button", { name: "Hide guide" });
      expect(hide).toHaveAttribute("aria-expanded", "true");

      fireEvent.click(hide);
      expect(
        screen.queryByRole("region", { name: "How to use LiveDub Voice" }),
      ).not.toBeInTheDocument();
    });
  });
});
