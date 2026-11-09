import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("renders the product name and the version from app_info", async () => {
    invokeMock.mockResolvedValue({
      name: "LiveDub Voice",
      version: "1.2.3",
      profile: "debug",
    });

    render(<App />);

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "LiveDub Voice",
    );
    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("app_info");
  });

  it("shows the error message when app_info fails", async () => {
    invokeMock.mockRejectedValue({ code: "internal", message: "boom" });

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
  });
});
