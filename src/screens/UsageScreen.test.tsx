import { act, fireEvent, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CapStatus, Settings, UsageSnapshot } from "../lib/api";
import { render } from "../test/i18n";
import UsageScreen from "./UsageScreen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

const NO_CAP: CapStatus = {
  dailyUsd: null,
  monthlyUsd: null,
  level: "off",
  period: null,
  spentUsd: 0,
  limitUsd: null,
  overriddenToday: false,
};

const SETTINGS: Settings = {
  version: 3,
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
    outputDeviceId: "cable",
  },
  spendingCap: { dailyUsd: 2, monthlyUsd: null },
};

function snapshot(overrides: Partial<UsageSnapshot> = {}): UsageSnapshot {
  return {
    pricePerMinuteUsd: 0.034,
    session: {
      incoming: { active: true, seconds: 150, costUsd: 0.085 },
      outgoing: { active: false, seconds: 0, costUsd: 0 },
    },
    today: { incomingSeconds: 150, outgoingSeconds: 60, costUsd: 0.119 },
    month: { incomingSeconds: 1800, outgoingSeconds: 1200, costUsd: 1.7 },
    days: [
      {
        date: "2026-11-20",
        incomingSeconds: 150,
        outgoingSeconds: 60,
        costUsd: 0.119,
      },
      {
        date: "2026-11-19",
        incomingSeconds: 1650,
        outgoingSeconds: 1140,
        costUsd: 1.581,
      },
    ],
    cap: NO_CAP,
    ...overrides,
  };
}

let handlers: Map<string, EventCallback<unknown>>;

function emit(name: string, payload: unknown) {
  const handler = handlers.get(name);
  if (!handler) throw new Error(`nobody listens to ${name}`);
  act(() => handler({ event: name, id: 1, payload }));
}

function setup(usage: UsageSnapshot, settings: Settings = SETTINGS) {
  invokeMock.mockImplementation((command, args) => {
    switch (command) {
      case "get_usage":
        return Promise.resolve(usage);
      case "get_settings":
        return Promise.resolve(settings);
      case "save_settings":
        return Promise.resolve((args as { settings: Settings }).settings);
      case "override_spending_cap":
        return Promise.resolve(
          snapshot({ cap: { ...usage.cap, overriddenToday: true } }),
        );
      default:
        return new Promise(() => {});
    }
  });
  render(<UsageScreen />);
}

beforeEach(() => {
  invokeMock.mockReset();
  vi.mocked(openUrl).mockClear();
  handlers = new Map();
  listenMock.mockReset();
  listenMock.mockImplementation((name, handler) => {
    handlers.set(name, handler);
    return Promise.resolve(() => {});
  });
});

describe("UsageScreen", () => {
  it("says the numbers are an estimate, not the OpenAI bill", async () => {
    setup(snapshot());

    expect(
      await screen.findByText(/This is not your OpenAI bill/),
    ).toHaveTextContent("$0.034 per audio minute");
  });

  it("shows each direction's session, today, this month and the days", async () => {
    setup(snapshot());

    const incoming = (
      await screen.findByText("Incoming · others → you")
    ).closest("section") as HTMLElement;
    expect(within(incoming).getByText("Live")).toBeInTheDocument();
    expect(within(incoming).getByText("2.5")).toBeInTheDocument();
    expect(within(incoming).getByText("$0.085")).toBeInTheDocument();

    const outgoing = screen
      .getByText("Outgoing · you → others")
      .closest("section") as HTMLElement;
    expect(
      within(outgoing).getByText("Not started since LiveDub Voice opened."),
    ).toBeInTheDocument();

    expect(screen.getByText("$0.119", { selector: "dd" })).toBeInTheDocument();
    expect(screen.getByText("$1.70")).toBeInTheDocument();
    expect(screen.getByText("30.0 min")).toBeInTheDocument();

    const rows = within(screen.getByRole("table")).getAllByRole("row");
    // Header, then newest first.
    expect(rows).toHaveLength(3);
    expect(rows[1]).toHaveTextContent("Nov 20, 2026");
    expect(rows[1]).toHaveTextContent("2.5 min");
    expect(rows[1]).toHaveTextContent("1.0 min");
    expect(rows[2]).toHaveTextContent("Nov 19, 2026");
    expect(rows[2]).toHaveTextContent("$1.58");
  });

  it("follows live usage updates", async () => {
    setup(snapshot());
    await screen.findByText("$0.085");

    emit(
      "usage-updated",
      snapshot({
        session: {
          incoming: { active: true, seconds: 210, costUsd: 0.119 },
          outgoing: { active: false, seconds: 0, costUsd: 0 },
        },
      }),
    );

    expect(screen.getByText("3.5")).toBeInTheDocument();
  });

  it("shows an empty state before any usage", async () => {
    setup(
      snapshot({
        session: {
          incoming: { active: false, seconds: 0, costUsd: 0 },
          outgoing: { active: false, seconds: 0, costUsd: 0 },
        },
        today: { incomingSeconds: 0, outgoingSeconds: 0, costUsd: 0 },
        month: { incomingSeconds: 0, outgoingSeconds: 0, costUsd: 0 },
        days: [],
      }),
    );

    expect(await screen.findByText("No usage yet")).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("warns from 80 % of the cap", async () => {
    setup(
      snapshot({
        cap: {
          ...NO_CAP,
          dailyUsd: 2,
          level: "warning",
          period: "daily",
          spentUsd: 1.7,
          limitUsd: 2,
        },
      }),
    );

    expect(
      await screen.findByText(
        "You have used 85% of your daily spending cap ($1.70 of $2.00).",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Keep going today" }),
    ).not.toBeInTheDocument();
  });

  it("says the cap stopped the translation and lets the user keep going today", async () => {
    const reached: CapStatus = {
      ...NO_CAP,
      monthlyUsd: 10,
      level: "reached",
      period: "monthly",
      spentUsd: 10.02,
      limitUsd: 10,
    };
    setup(snapshot({ cap: reached }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "You reached your monthly spending cap ($10.02 of $10.00).",
    );
    emit("spending-cap-reached", reached);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Translation stopped: you reached your monthly spending cap",
    );

    fireEvent.click(screen.getByRole("button", { name: "Keep going today" }));

    expect(
      await screen.findByText(/translating anyway until midnight/),
    ).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("override_spending_cap");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("SpendingCapCard", () => {
  function card() {
    return screen.getByRole("region", { name: "Spending cap" });
  }

  it("loads the caps, off by default as empty fields", async () => {
    setup(snapshot());

    const daily = await within(card()).findByLabelText("Daily cap (USD)");
    await vi.waitFor(() => expect(daily).toHaveValue("2"));
    expect(within(card()).getByLabelText("Monthly cap (USD)")).toHaveValue("");
  });

  it("saves the caps into the settings", async () => {
    setup(snapshot());
    const daily = await within(card()).findByLabelText("Daily cap (USD)");
    await vi.waitFor(() => expect(daily).not.toBeDisabled());

    fireEvent.change(daily, { target: { value: "" } });
    fireEvent.change(within(card()).getByLabelText("Monthly cap (USD)"), {
      target: { value: "0,01" },
    });
    fireEvent.click(within(card()).getByRole("button", { name: "Save cap" }));

    expect(
      await within(card()).findByText("Spending cap saved."),
    ).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("save_settings", {
      settings: {
        ...SETTINGS,
        spendingCap: { dailyUsd: null, monthlyUsd: 0.01 },
      },
    });
  });

  it("refuses amounts out of range without saving", async () => {
    setup(snapshot());
    const daily = await within(card()).findByLabelText("Daily cap (USD)");
    await vi.waitFor(() => expect(daily).not.toBeDisabled());

    for (const bad of ["0", "-3", "abc", "20000"]) {
      fireEvent.change(daily, { target: { value: bad } });
      fireEvent.click(within(card()).getByRole("button", { name: "Save cap" }));
      expect(
        await within(card()).findByText(/Enter an amount between 0.01/),
      ).toBeInTheDocument();
    }
    expect(daily).toHaveAttribute("aria-invalid", "true");
    expect(invokeMock).not.toHaveBeenCalledWith(
      "save_settings",
      expect.anything(),
    );
  });

  it("links to the budget in OpenAI's dashboard", async () => {
    setup(snapshot());

    fireEvent.click(
      await within(card()).findByRole("button", {
        name: "Set a budget in your OpenAI account",
      }),
    );

    expect(openUrl).toHaveBeenCalledWith(
      "https://platform.openai.com/settings/organization/limits",
    );
  });
});
