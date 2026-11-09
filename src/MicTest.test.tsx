import { act, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MicTest from "./MicTest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

/** Event handlers the component registered, by event name. */
let handlers: Map<string, EventCallback<unknown>>;
const unlisten = vi.fn();

function emit(name: string, payload: unknown) {
  const handler = handlers.get(name);
  if (!handler) throw new Error(`nobody listens to ${name}`);
  act(() => handler({ event: name, id: 1, payload }));
}

function commands(): [string, unknown][] {
  return invokeMock.mock.calls.map(([command, args]) => [command, args]);
}

function lastCommand(): [string, unknown] | undefined {
  const all = commands();
  return all[all.length - 1];
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  handlers = new Map();
  unlisten.mockReset();
  listenMock.mockReset();
  listenMock.mockImplementation((name, handler) => {
    handlers.set(name, handler);
    return Promise.resolve(unlisten);
  });
});

describe("MicTest", () => {
  it("does nothing until the test starts", () => {
    render(<MicTest inputId="mic" outputId="spk" />);

    expect(
      screen.getByRole("meter", { name: "Microphone level" }),
    ).toHaveAttribute("aria-valuenow", "0");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });

  it("starts the monitor and shows the level", async () => {
    render(<MicTest inputId="mic" outputId="spk" />);

    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await screen.findByRole("button", { name: "Stop test" });
    await vi.waitFor(() =>
      expect(commands()).toEqual([
        ["start_monitor", { inputId: "mic", outputId: "spk", listen: false }],
      ]),
    );

    emit("monitor-level", { rms: 0.1, peak: 0.5 });
    // 0.1 = -20 dBFS: two thirds of the way from -60 to 0.
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "67");
  });

  it("restarts with playback when listening is turned on, and stops on stop", async () => {
    render(<MicTest inputId={null} outputId={null} />);
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));

    fireEvent.click(screen.getByRole("switch", { name: "Listen to myself" }));
    await vi.waitFor(() =>
      expect(commands()).toContainEqual([
        "start_monitor",
        { inputId: null, outputId: null, listen: true },
      ]),
    );

    fireEvent.click(screen.getByRole("button", { name: "Stop test" }));
    await vi.waitFor(() =>
      expect(lastCommand()).toEqual(["stop_monitor", undefined]),
    );
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(2));
  });

  it("shows why the monitor could not start", async () => {
    invokeMock.mockImplementation((command) =>
      command === "start_monitor"
        ? // Tauri rejects failing commands with a plain `{ code, message }` object.
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({
            code: "audio",
            message:
              "audio device error: formats differ, needs resampling: the audio is 44100 Hz, 1 ch but the output device plays 48000 Hz, 2 ch",
          })
        : Promise.resolve(undefined),
    );
    render(<MicTest inputId="mic" outputId="spk" />);

    fireEvent.click(screen.getByRole("button", { name: "Start test" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "formats differ, needs resampling",
    );
    expect(
      screen.getByRole("button", { name: "Start test" }),
    ).toBeInTheDocument();
  });

  it("shows why the monitor stopped on its own", async () => {
    render(<MicTest inputId="mic" outputId="spk" />);
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await vi.waitFor(() => expect(handlers.has("monitor-stopped")).toBe(true));

    emit("monitor-stopped", {
      message:
        "audio device error: the microphone stopped: the device was unplugged or disabled",
    });

    expect(screen.getByRole("alert")).toHaveTextContent("unplugged");
    expect(
      screen.getByRole("button", { name: "Start test" }),
    ).toBeInTheDocument();
  });

  it("stops the monitor when it goes away", async () => {
    const { unmount } = render(<MicTest inputId="mic" outputId="spk" />);
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled());

    unmount();

    await vi.waitFor(() =>
      expect(lastCommand()).toEqual(["stop_monitor", undefined]),
    );
  });
});
