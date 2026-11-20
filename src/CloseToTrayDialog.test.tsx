import { act, fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CloseToTrayDialog from "./CloseToTrayDialog";
import { render } from "./test/i18n";
import { useTrayLabels } from "./useTrayLabels";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);
let handlers: Map<string, EventCallback<unknown>>;

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  handlers = new Map();
  listenMock.mockReset();
  listenMock.mockImplementation((name, handler) => {
    handlers.set(name, handler);
    return Promise.resolve(() => {});
  });
});

async function closeWindow() {
  await vi.waitFor(() =>
    expect(handlers.has("close-to-tray-notice")).toBe(true),
  );
  act(() =>
    handlers.get("close-to-tray-notice")?.({
      event: "close-to-tray-notice",
      id: 1,
      payload: null,
    }),
  );
}

describe("CloseToTrayDialog", () => {
  it("stays hidden until the window is closed", () => {
    render(<CloseToTrayDialog />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("explains the tray the first time the window closes, then hides it", async () => {
    render(<CloseToTrayDialog />);
    await closeWindow();

    const dialog = screen.getByRole("dialog", {
      name: "LiveDub Voice keeps running",
    });
    expect(dialog).toHaveTextContent(/the translations keep running/);
    const hide = screen.getByRole("button", {
      name: "Got it, hide to the tray",
    });
    expect(hide).toHaveFocus();

    fireEvent.click(hide);

    expect(invokeMock).toHaveBeenCalledWith("hide_to_tray");
    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("can quit instead", async () => {
    render(<CloseToTrayDialog />);
    await closeWindow();

    fireEvent.click(screen.getByRole("button", { name: "Quit LiveDub Voice" }));

    expect(invokeMock).toHaveBeenCalledWith("quit_app");
  });

  it("closes on Escape without hiding the window", async () => {
    render(<CloseToTrayDialog />);
    await closeWindow();

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("hide_to_tray");
  });

  it("is in Spanish when the panel is", async () => {
    render(<CloseToTrayDialog />, { uiLanguage: "es" });
    await closeWindow();

    expect(
      screen.getByRole("dialog", { name: "LiveDub Voice sigue activo" }),
    ).toBeInTheDocument();
  });
});

function TrayLabels() {
  useTrayLabels();
  return null;
}

describe("useTrayLabels", () => {
  it("sends the tray menu's strings in the panel language", async () => {
    render(<TrayLabels />, { uiLanguage: "es" });

    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("set_tray_labels", {
        labels: {
          open: "Abrir LiveDub Voice",
          quit: "Salir de LiveDub Voice",
          incoming: "Entrante (los demás → tu idioma)",
          outgoing: "Saliente (tu voz → los demás)",
          states: {
            idle: "Apagada",
            connecting: "Conectando…",
            live: "En vivo",
            reconnecting: "Reconectando…",
            stopping: "Terminando…",
            error: "Error",
          },
        },
      }),
    );
  });
});
