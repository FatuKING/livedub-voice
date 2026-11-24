import { act, fireEvent, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "../test/i18n";
import ShortcutField from "./ShortcutField";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

function field() {
  return screen.getByRole("button", { name: "Turn incoming on or off" });
}

function suspendedCalls(): boolean[] {
  return invokeMock.mock.calls
    .filter(([command]) => command === "set_hotkeys_suspended")
    .map(([, args]) => (args as { suspended: boolean }).suspended);
}

function setup(
  value: string | null = "Ctrl+Alt+KeyI",
  validate?: (value: string) => string | null,
) {
  const onChange = vi.fn();
  const { unmount } = render(
    <ShortcutField
      label="Turn incoming on or off"
      value={value}
      onChange={onChange}
      validate={validate}
    />,
  );
  return { onChange, unmount };
}

/** Presses a key on the focused recorder. */
async function press(
  code: string,
  mods: Partial<
    Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>
  > = {},
  key = code,
) {
  fireEvent.keyDown(field(), { code, key, ...mods });
  // Let the core's check settle.
  await act(async () => {});
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

describe("ShortcutField", () => {
  it("shows the shortcut as the user reads it, or None", () => {
    setup();
    expect(field()).toHaveTextContent("Ctrl + Alt + I");
  });

  it("shows None without a shortcut and cannot clear it", () => {
    setup(null);
    expect(field()).toHaveTextContent("None");
    expect(
      screen.getByRole("button", { name: "Clear the shortcut" }),
    ).toBeDisabled();
  });

  it("records a combination, checks it with the core and turns shortcuts back on", async () => {
    const { onChange } = setup();

    fireEvent.click(field());
    expect(field()).toHaveTextContent("Press the keys… (Esc to cancel)");
    expect(field()).toHaveAttribute("aria-pressed", "true");

    // Modifiers alone keep waiting, showing what is held.
    await press("ControlLeft", { ctrlKey: true }, "Control");
    await press("AltLeft", { ctrlKey: true, altKey: true }, "Alt");
    expect(field()).toHaveTextContent("Ctrl + Alt");
    expect(onChange).not.toHaveBeenCalled();

    await press("KeyK", { ctrlKey: true, altKey: true }, "k");

    expect(invokeMock).toHaveBeenCalledWith("check_hotkey", {
      shortcut: "Ctrl+Alt+KeyK",
    });
    expect(onChange).toHaveBeenCalledWith("Ctrl+Alt+KeyK");
    expect(suspendedCalls()).toEqual([true, false]);
  });

  it("refuses a typing key without Ctrl, Alt or Win", async () => {
    const { onChange } = setup();

    fireEvent.click(field());
    await press("KeyT", { shiftKey: true }, "T");

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Shift + T needs Ctrl, Alt or Win",
    );
    expect(onChange).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalledWith(
      "check_hotkey",
      expect.anything(),
    );
    expect(suspendedCalls()).toEqual([true, false]);
  });

  it("accepts a function key on its own", async () => {
    const { onChange } = setup();

    fireEvent.click(field());
    await press("F9");

    expect(onChange).toHaveBeenCalledWith("F9");
  });

  it("shows the form's conflict and keeps the old shortcut", async () => {
    const { onChange } = setup("Ctrl+Alt+KeyI", (value) =>
      value === "Ctrl+Alt+KeyO" ? "Already used." : null,
    );

    fireEvent.click(field());
    await press("KeyO", { ctrlKey: true, altKey: true });

    expect(screen.getByRole("alert")).toHaveTextContent("Already used.");
    expect(onChange).not.toHaveBeenCalled();
    expect(field()).toHaveTextContent("Ctrl + Alt + I");
  });

  it("explains a combination that types a character on the layout", async () => {
    invokeMock.mockImplementation((command) =>
      command === "check_hotkey"
        ? // Tauri rejects failing commands with a plain `{ code, message }` object.
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({
            code: "hotkey_types_character",
            message:
              "shortcut types a character on this keyboard layout: Ctrl+Alt+KeyQ",
          })
        : Promise.resolve(undefined),
    );
    const { onChange } = setup();

    fireEvent.click(field());
    await press("KeyQ", { ctrlKey: true, altKey: true });

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Ctrl + Alt + Q types a character on your keyboard layout (AltGr)",
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("cancels with Escape and on blur", async () => {
    const { onChange } = setup();

    fireEvent.click(field());
    await press("Escape");
    expect(field()).toHaveTextContent("Ctrl + Alt + I");
    expect(field()).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(field());
    fireEvent.blur(field());
    expect(field()).toHaveAttribute("aria-pressed", "false");

    expect(onChange).not.toHaveBeenCalled();
    expect(suspendedCalls()).toEqual([true, false, true, false]);
  });

  it("clears the shortcut", () => {
    const { onChange } = setup();

    fireEvent.click(screen.getByRole("button", { name: "Clear the shortcut" }));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("turns the shortcuts back on when it goes away mid-recording", () => {
    const { unmount } = setup();
    fireEvent.click(field());
    expect(suspendedCalls()).toEqual([true]);

    // Leaving Settings while recording must not leave the shortcuts off.
    unmount();

    expect(suspendedCalls()).toEqual([true, false]);
  });
});
