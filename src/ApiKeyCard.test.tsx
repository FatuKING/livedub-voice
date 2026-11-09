import { fireEvent, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ApiKeyCard from "./ApiKeyCard";
import type { ApiKeyStatus, KeyValidation, SaveApiKeyResult } from "./lib/api";
import { render } from "./test/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

/** Fake key: never a real one. */
const FAKE_KEY = "sk-test-0000000000000000000000abcd";

const NO_KEY: ApiKeyStatus = { hasKey: false, last4: null };
const SAVED: ApiKeyStatus = { hasKey: true, last4: "abcd" };

interface Backend {
  status?: ApiKeyStatus;
  validation?: KeyValidation;
  save?: () => Promise<SaveApiKeyResult>;
}

function mockBackend({
  status = NO_KEY,
  validation = { kind: "valid" },
  save,
}: Backend) {
  invokeMock.mockImplementation((command) => {
    switch (command) {
      case "get_api_key_status":
        return Promise.resolve(status);
      case "validate_api_key":
        return Promise.resolve(validation);
      case "save_api_key":
        return save ? save() : Promise.resolve({ status: SAVED, validation });
      case "delete_api_key":
        return Promise.resolve(NO_KEY);
      default:
        return new Promise(() => {});
    }
  });
}

function card() {
  return screen.getByRole("region", { name: "OpenAI API key" });
}

describe("ApiKeyCard", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("shows Not set when no key is stored, without checking", async () => {
    mockBackend({ status: NO_KEY });

    render(<ApiKeyCard />);

    expect(await within(card()).findByText("Not set")).toBeInTheDocument();
    expect(screen.queryByText(/Saved key/)).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("validate_api_key");
    expect(
      screen.getByRole("button", { name: "Save and validate" }),
    ).toBeDisabled();
  });

  it("checks a stored key on load and shows its last four characters", async () => {
    mockBackend({ status: SAVED, validation: { kind: "noModelAccess" } });

    render(<ApiKeyCard />);

    expect(
      await within(card()).findByText("No model access"),
    ).toBeInTheDocument();
    expect(screen.getByText("sk-…abcd")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      /cannot use gpt-realtime-translate/,
    );
    expect(invokeMock).toHaveBeenCalledWith("validate_api_key");
  });

  it("saves the key, shows the result and clears the input", async () => {
    mockBackend({ status: NO_KEY, validation: { kind: "valid" } });
    render(<ApiKeyCard />);
    await within(card()).findByText("Not set");

    const input = screen.getByLabelText("API key");
    expect(input).toHaveAttribute("type", "password");
    fireEvent.change(input, { target: { value: FAKE_KEY } });
    fireEvent.click(screen.getByRole("button", { name: "Save and validate" }));

    expect(await within(card()).findByText("Valid")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("save_api_key", { key: FAKE_KEY });
    expect(screen.getByText("sk-…abcd")).toBeInTheDocument();
    expect(screen.getByLabelText("Replace API key")).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent(
      /a key with no credits also passes/,
    );
  });

  it("keeps the input and shows the error when the key is rejected", async () => {
    mockBackend({
      status: NO_KEY,
      save: () =>
        // Tauri rejects failing commands with a plain `{ code, message }` object.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        Promise.reject({
          code: "invalid_api_key",
          message: 'invalid API key: OpenAI API keys start with "sk-"',
        }),
    });
    render(<ApiKeyCard />);
    await within(card()).findByText("Not set");

    fireEvent.change(screen.getByLabelText("API key"), {
      target: { value: "not-a-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save and validate" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save the key: This does not look like an OpenAI API key",
    );
    expect(screen.getByLabelText("API key")).toHaveValue("not-a-key");
    expect(within(card()).getByText("Not set")).toBeInTheDocument();
  });

  it("reports offline but keeps the key saved", async () => {
    mockBackend({ status: NO_KEY, validation: { kind: "offline" } });
    render(<ApiKeyCard />);
    await within(card()).findByText("Not set");

    fireEvent.change(screen.getByLabelText("API key"), {
      target: { value: FAKE_KEY },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save and validate" }));

    expect(await within(card()).findByText("Offline")).toBeInTheDocument();
    expect(screen.getByText("sk-…abcd")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(/The key is saved/);
  });

  it("toggles the key visibility", async () => {
    mockBackend({ status: NO_KEY });
    render(<ApiKeyCard />);
    await within(card()).findByText("Not set");

    fireEvent.click(screen.getByRole("button", { name: "Show key" }));

    expect(screen.getByLabelText("API key")).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "Hide key" }));
    expect(screen.getByLabelText("API key")).toHaveAttribute(
      "type",
      "password",
    );
  });

  it("asks for confirmation before removing the key", async () => {
    mockBackend({ status: SAVED, validation: { kind: "valid" } });
    render(<ApiKeyCard />);
    await within(card()).findByText("Valid");

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(invokeMock).not.toHaveBeenCalledWith("delete_api_key");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("button", { name: "Yes, remove key" }),
    ).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("delete_api_key");

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    fireEvent.click(screen.getByRole("button", { name: "Yes, remove key" }));

    expect(await within(card()).findByText("Not set")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("delete_api_key");
    expect(screen.queryByText("sk-…abcd")).not.toBeInTheDocument();
  });
});
