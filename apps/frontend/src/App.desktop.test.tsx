import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { cancelQuickScan, startQuickScan } from "./api";

vi.mock("./api", () => ({
  bootstrap: vi.fn().mockResolvedValue({ mode: "desktop", pending: [], inventory: [] }),
  cancelQuickScan: vi.fn(),
  review: vi.fn(),
  startQuickScan: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(startQuickScan).mockReset();
  vi.mocked(cancelQuickScan).mockReset();
});
afterEach(cleanup);

describe("desktop scan recovery", () => {
  it("recovers when a scan cannot start", async () => {
    vi.mocked(startQuickScan).mockRejectedValue(new Error("busy"));
    const user = userEvent.setup();
    render(<App />);

    const button = await screen.findByRole("button", { name: /run quick scan/i });
    await user.click(button);

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not start/i);
    expect(button).toBeEnabled();
  });

  it("surfaces a nonterminal scanner warning", async () => {
    vi.mocked(startQuickScan).mockImplementation(async (onEvent) => {
      onEvent({ kind: "scanner_failed", message: "A discovery could not be saved" });
      onEvent({ kind: "completed", visited: 10, discovered: 0 });
      return { id: "scan-1", unlisten: vi.fn() };
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be saved/i);
  });

  it("shows the scanner identity for progress events", async () => {
    vi.mocked(startQuickScan).mockImplementation(async (onEvent) => {
      onEvent({ kind: "progress", scanner_id: "windows.process", visited: 7 });
      return { id: "scan-1", unlisten: vi.fn() };
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    const progress = await screen.findByLabelText(/scan progress/i);
    expect(progress).toHaveTextContent("windows.process | 7 locations checked");
  });

  it("disables cancellation until the backend returns a scan id", async () => {
    vi.mocked(startQuickScan).mockImplementation(() => new Promise(() => undefined));
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    expect(await screen.findByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("cleans up when backend cancellation fails", async () => {
    vi.mocked(startQuickScan).mockResolvedValue({ id: "scan-1", unlisten: vi.fn() });
    vi.mocked(cancelQuickScan).mockRejectedValue(new Error("already ended"));
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be confirmed/i);
    expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /run quick scan/i })).toBeEnabled();
  });
});
