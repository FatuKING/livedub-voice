import { act, fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TranslationControls, {
  type TranslationControlsProps,
} from "./TranslationControls";
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

const OUTGOING_HELP =
  'In Discord / WhatsApp Web choose "CABLE Output" as your microphone.';

function renderControls(props: Partial<TranslationControlsProps> = {}) {
  const onOpenSettings = vi.fn();
  render(
    <TranslationControls
      direction="outgoing"
      blocked={null}
      help={OUTGOING_HELP}
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
    command === "get_outgoing_status" || command === "get_incoming_status"
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

describe("TranslationControls (outgoing)", () => {
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
    expect(screen.getByText(OUTGOING_HELP)).toBeInTheDocument();
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

  it("shows the reconnect attempt and can stop while reconnecting", async () => {
    currentStatus = { state: "live", error: null };
    renderControls();
    await screen.findByText("Live");

    emitStatus({
      state: "reconnecting",
      error: null,
      reconnect: { attempt: 2, maxAttempts: 6 },
    });

    expect(screen.getByText("Reconnecting 2/6…")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Lost the connection to OpenAI. Reconnecting… Until then others hear nothing from you.",
    );
    const stop = screen.getByRole("button", { name: "Stop outgoing" });
    expect(stop).toBeEnabled();

    emitStatus({ state: "live", error: null, reconnect: null });
    expect(screen.getByText("Live")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("explains a failed reconnect", async () => {
    currentStatus = { state: "live", error: null };
    renderControls();
    await screen.findByText("Live");

    emitStatus({
      state: "error",
      error: {
        code: "reconnect_failed",
        message:
          "lost the connection to OpenAI and could not reconnect after 6 attempts: could not reach OpenAI: connection refused",
      },
      reconnect: null,
    });

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The outgoing translation stopped: Lost the connection to OpenAI and could not reconnect (could not reach OpenAI: connection refused). Check your connection and start again.",
    );
    expect(
      screen.getByRole("button", { name: "Start outgoing" }),
    ).toBeEnabled();
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

  it("disables Start and explains why", async () => {
    const text = "Add your OpenAI API key in Settings to start.";
    const { onOpenSettings } = renderControls({ blocked: text });
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
    renderControls({
      blocked: "Add your OpenAI API key in Settings to start.",
    });

    expect(
      await screen.findByRole("button", { name: "Stop outgoing" }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Go to Settings" }),
    ).not.toBeInTheDocument();
  });

  it("shows a warning that does not block Start", async () => {
    renderControls({
      warning:
        "The output in Settings is not a known virtual cable. Others only hear you if their app records from it.",
    });
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
      <TranslationControls
        direction="outgoing"
        blocked={null}
        help={OUTGOING_HELP}
        onOpenSettings={vi.fn()}
      />,
    );
    await subscribed();

    view.unmount();

    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(3));
    expect(invokeMock).not.toHaveBeenCalledWith("stop_outgoing");
  });
});

describe("TranslationControls (incoming)", () => {
  function renderIncoming(props: Partial<TranslationControlsProps> = {}) {
    return renderControls({
      direction: "incoming",
      help: "While it runs, the original audio plays at 20% under the translation.",
      ...props,
    });
  }

  it("starts and stops the incoming translation, with its own level and captions", async () => {
    renderIncoming();
    await subscribed();

    expect(screen.getByText("Off")).toBeInTheDocument();
    expect(screen.getByRole("meter", { name: "App audio" })).toHaveAttribute(
      "aria-valuenow",
      "0",
    );
    expect(
      screen.getByText("The translation of what others say shows up here."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Start incoming" }));
    expect(invokeMock).toHaveBeenCalledWith("start_incoming");
    emit("incoming-status", { state: "live", error: null });
    expect(await screen.findByText("Live")).toBeInTheDocument();

    emit("incoming-level", { rms: 0.1, peak: 0.5 });
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "67");
    emit("incoming-transcript", { delta: "Hola" });
    emit("incoming-transcript", { delta: " a todos" });
    expect(screen.getByText("Hola a todos")).toBeInTheDocument();
    expect(screen.getByText("What you hear")).toBeInTheDocument();

    await vi.waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Stop incoming" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop incoming" }));
    await screen.findByRole("button", { name: "Start incoming" });
    expect(invokeMock).toHaveBeenCalledWith("stop_incoming");
    expect(invokeMock).not.toHaveBeenCalledWith("start_outgoing");
  });

  it("ignores the outgoing direction's events", async () => {
    renderIncoming();
    await subscribed();

    expect(handlers.has("outgoing-status")).toBe(false);
    expect([...handlers.keys()].sort()).toEqual([
      "incoming-level",
      "incoming-status",
      "incoming-transcript",
    ]);
  });

  it("explains why the app could not be captured", async () => {
    invokeMock.mockImplementation((command) => {
      if (command === "get_incoming_status")
        return Promise.resolve(currentStatus);
      if (command === "start_incoming")
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          code: "source_app_not_running",
          message: "the chosen app is not running: Discord.exe",
        });
      return Promise.resolve(undefined);
    });
    renderIncoming();
    await subscribed();

    fireEvent.click(screen.getByRole("button", { name: "Start incoming" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The incoming translation stopped: The app chosen in Settings is not running",
    );
  });
});

describe("TranslationControls and the spending cap", () => {
  /** `start_outgoing` is refused by the cap until `override_spending_cap` is called. */
  function mockCap() {
    let overridden = false;
    invokeMock.mockImplementation((command) => {
      switch (command) {
        case "get_outgoing_status":
          return Promise.resolve(currentStatus);
        case "override_spending_cap":
          overridden = true;
          return Promise.resolve({});
        case "start_outgoing":
          return overridden
            ? Promise.resolve(undefined)
            : // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
              Promise.reject({
                code: "spending_cap_reached",
                message:
                  "the daily spending cap is reached: spent 1.00 of 1.00 USD",
              });
        default:
          return Promise.resolve(undefined);
      }
    });
  }

  it("asks before starting over the cap, and keeps going today on request", async () => {
    mockCap();
    const onOpenUsage = vi.fn();
    renderControls({ onOpenUsage });
    await subscribed();

    fireEvent.click(screen.getByRole("button", { name: "Start outgoing" }));

    const question = await screen.findByRole("group", {
      name: "Spending cap reached",
    });
    expect(question).toHaveTextContent(
      "Your spending cap is reached. Keep translating for the rest of today?",
    );
    // Not an error: nothing started.
    expect(screen.getByText("Off")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Raise the cap" }));
    expect(onOpenUsage).toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Keep going today" }));

    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("start_outgoing"),
    );
    expect(invokeMock).toHaveBeenCalledWith("override_spending_cap");
    expect(
      screen.queryByRole("group", { name: "Spending cap reached" }),
    ).not.toBeInTheDocument();
    expect(await screen.findByText("Connecting…")).toBeInTheDocument();
  });

  it("can cancel instead", async () => {
    mockCap();
    renderControls();
    await subscribed();

    fireEvent.click(screen.getByRole("button", { name: "Start outgoing" }));
    await screen.findByRole("group", { name: "Spending cap reached" });
    // No Usage screen to open from here.
    expect(
      screen.queryByRole("button", { name: "Raise the cap" }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(
      screen.queryByRole("group", { name: "Spending cap reached" }),
    ).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("override_spending_cap");
  });
});
