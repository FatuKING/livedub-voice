import { act, fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import OutgoingControls, {
  type OutgoingControlsProps,
} from "./OutgoingControls";
import type { PipelineStatus } from "./lib/api";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

let handlers: Map<string, EventCallback<unknown>>;
const unlisten = vi.fn();
let currentStatus: PipelineStatus;

function emit(name: string, payload: unknown) {
  const handler = handlers.get(name);
  if (!handler) throw new Error(`nobody listens to ${name}`);
  act(() => handler({ event: name, id: 1, payload }));
}

function emitStatus(status: PipelineStatus) {
  emit("outgoing-status", status);
}

function renderControls(props: Partial<OutgoingControlsProps> = {}) {
  const onOpenSettings = vi.fn();
  render(
    <OutgoingControls
      blocker={null}
      outputIsCable
      onOpenSettings={onOpenSettings}
      {...props}
    />,
  );
  return { onOpenSettings };
}

async function subscribed() {
  await vi.waitFor(() => expect(handlers.size).toBe(3));
}

beforeEach(() => {
  currentStatus = { state: "idle", error: null };
  invokeMock.mockReset();
  invokeMock.mockImplementation((command) =>
    command === "get_outgoing_status"
      ? Promise.resolve(currentStatus)
      : Promise.resolve(undefined),
  );
  handlers = new Map();
  unlisten.mockReset();
  listenMock.mockReset();
  listenMock.mockImplementation((name, handler) => {
    handlers.set(name, handler);
    return Promise.resolve(unlisten);
  });
});

describe("OutgoingControls", () => {
  it("starts idle with an empty caption area", async () => {
    renderControls();
    await subscribed();

    expect(screen.getByText("Off")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeEnabled();
    expect(
      screen.getByText("The translation of what you say shows up here."),
    ).toBeInTheDocument();
    expect(screen.getByRole("meter", { name: "Microphone" })).toHaveAttribute(
      "aria-valuenow",
      "0",
    );
    expect(
      screen.getByText(
        'In Discord / WhatsApp Web choose "CABLE Output" as your microphone.',
      ),
    ).toBeInTheDocument();
  });

  it("goes through connecting and live, showing the level and captions", async () => {
    // The command resolves only once the pipeline is live.
    let resolveStart: () => void = () => {};
    invokeMock.mockImplementation((command) => {
      if (command === "get_outgoing_status")
        return Promise.resolve(currentStatus);
      if (command === "start_outgoing")
        return new Promise<void>((resolve) => (resolveStart = resolve));
      return Promise.resolve(undefined);
    });
    renderControls();
    await subscribed();

    fireEvent.click(screen.getByRole("button", { name: "Start outgoing" }));
    expect(invokeMock).toHaveBeenCalledWith("start_outgoing");
    expect(screen.getByText("Connecting…")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Stop outgoing" }),
    ).toBeDisabled();

    emitStatus({ state: "live", error: null });
    act(() => resolveStart());
    expect(screen.getByText("Live")).toBeInTheDocument();
    await vi.waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Stop outgoing" }),
      ).toBeEnabled(),
    );

    emit("outgoing-level", { rms: 0.1, peak: 0.5 });
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "67");

    emit("outgoing-transcript", { delta: "Hello" });
    emit("outgoing-transcript", { delta: " everyone," });
    expect(screen.getByText("Hello everyone,")).toBeInTheDocument();
  });

  it("shows that it is finishing the translation while it stops", async () => {
    currentStatus = { state: "live", error: null };
    let resolveStop: () => void = () => {};
    invokeMock.mockImplementation((command) => {
      if (command === "get_outgoing_status")
        return Promise.resolve(currentStatus);
      if (command === "stop_outgoing")
        return new Promise<void>((resolve) => (resolveStop = resolve));
      return Promise.resolve(undefined);
    });
    renderControls();
    await screen.findByText("Live");

    fireEvent.click(screen.getByRole("button", { name: "Stop outgoing" }));
    expect(screen.getByText("Finishing translation…")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Stop outgoing" }),
    ).toBeDisabled();

    emitStatus({ state: "stopping", error: null });
    expect(
      screen.getByRole("button", { name: "Stop outgoing" }),
    ).toBeDisabled();

    emitStatus({ state: "idle", error: null });
    act(() => resolveStop());
    await screen.findByRole("button", { name: "Start outgoing" });
    expect(screen.getByText("Off")).toBeInTheDocument();
  });

  it("stops and returns to idle", async () => {
    currentStatus = { state: "live", error: null };
    renderControls();
    await screen.findByText("Live");

    fireEvent.click(screen.getByRole("button", { name: "Stop outgoing" }));

    await screen.findByRole("button", { name: "Start outgoing" });
    expect(invokeMock).toHaveBeenCalledWith("stop_outgoing");
    expect(screen.getByText("Off")).toBeInTheDocument();
  });

  it("shows why it could not start", async () => {
    invokeMock.mockImplementation((command) => {
      if (command === "get_outgoing_status")
        return Promise.resolve(currentStatus);
      if (command === "start_outgoing")
        // Tauri rejects failing commands with a plain `{ code, message }` object.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          code: "openai_auth",
          message: "OpenAI rejected the API key",
        });
      return Promise.resolve(undefined);
    });
    renderControls();
    await subscribed();

    fireEvent.click(screen.getByRole("button", { name: "Start outgoing" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The outgoing translation stopped: OpenAI rejected the API key.",
    );
    expect(screen.getByText("Error")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeEnabled();
  });

  it("shows why it stopped on its own, and resets the level", async () => {
    currentStatus = { state: "live", error: null };
    renderControls();
    await screen.findByText("Live");
    emit("outgoing-level", { rms: 0.1, peak: 0.5 });

    emitStatus({
      state: "error",
      error: {
        code: "audio",
        message:
          "audio device error: the microphone stopped: the device was unplugged or disabled",
      },
    });

    expect(screen.getByRole("alert")).toHaveTextContent("unplugged");
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeInTheDocument();
  });

  it.each([
    ["noApiKey", "Add your OpenAI API key in Settings to start."],
    [
      "noOutput",
      "Choose the virtual cable (for example CABLE Input) as the output in Settings to start.",
    ],
    [
      "outputMissing",
      "The output chosen in Settings is not connected. Install or plug in the virtual cable, or choose another output.",
    ],
  ] as const)("disables Start and explains why (%s)", async (blocker, text) => {
    const { onOpenSettings } = renderControls({ blocker });
    await subscribed();

    const start = screen.getByRole("button", { name: "Start outgoing" });
    expect(start).toBeDisabled();
    expect(start).toHaveAccessibleDescription(
      expect.stringContaining(text) as string,
    );

    fireEvent.click(screen.getByRole("button", { name: "Go to Settings" }));
    expect(onOpenSettings).toHaveBeenCalled();
  });

  it("lets a running translation be stopped even when Start would be blocked", async () => {
    currentStatus = { state: "live", error: null };
    renderControls({ blocker: "noApiKey" });

    expect(
      await screen.findByRole("button", { name: "Stop outgoing" }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Go to Settings" }),
    ).not.toBeInTheDocument();
  });

  it("warns when the output is not a known virtual cable", async () => {
    renderControls({ outputIsCable: false });
    await subscribed();

    expect(screen.getByRole("status")).toHaveTextContent(
      "not a known virtual cable",
    );
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeEnabled();
  });

  it("stops listening when it goes away, without stopping the translation", async () => {
    const view = render(
      <OutgoingControls
        blocker={null}
        outputIsCable
        onOpenSettings={vi.fn()}
      />,
    );
    await subscribed();

    view.unmount();

    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(3));
    expect(invokeMock).not.toHaveBeenCalledWith("stop_outgoing");
  });
});
