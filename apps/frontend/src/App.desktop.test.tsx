import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { cancelScan, pauseScan, pickScanRoots, resumeScan, reviewDiscovery, startScan } from "./api";

vi.mock("./api", () => ({
  bootstrap: vi.fn().mockResolvedValue({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r1" }),
  pickScanRoots: vi.fn(),
  startScan: vi.fn(),
  pauseScan: vi.fn(),
  resumeScan: vi.fn(),
  cancelScan: vi.fn(),
  reviewDiscovery: vi.fn(),
  isDesktop: vi.fn(() => true),
}));

beforeEach(() => {
  vi.mocked(startScan).mockReset();
  vi.mocked(pauseScan).mockReset();
  vi.mocked(resumeScan).mockReset();
  vi.mocked(cancelScan).mockReset();
  vi.mocked(pickScanRoots).mockReset();
  vi.mocked(reviewDiscovery).mockReset();
});
afterEach(cleanup);

describe("desktop scan recovery", () => {
  it("starts a quick scan with the exact camelCase ScanRequest shape", async () => {
    vi.mocked(startScan).mockImplementation(
      () =>
        new Promise(() => {
          // never resolves; we only assert on the call shape
        }),
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    expect(startScan).toHaveBeenCalledWith(
      expect.objectContaining({
        mode: "quick",
        roots: [],
        followReparsePoints: false,
        networkConsent: false,
        revision: "workspace-r1",
      }),
      expect.any(Function),
    );
  });

  it("recovers when a scan cannot start", async () => {
    vi.mocked(startScan).mockRejectedValue(new Error("busy"));
    const user = userEvent.setup();
    render(<App />);

    const button = await screen.findByRole("button", { name: /run quick scan/i });
    await user.click(button);

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not start/i);
    expect(button).toBeEnabled();
  });

  it("surfaces a nonterminal scanner warning", async () => {
    vi.mocked(startScan).mockImplementation(async (_request, onEvent) => {
      onEvent({ kind: "scanner_failed", scanner_id: "windows.process", code: "io", message: "A discovery could not be saved" });
      onEvent({ kind: "completed", visited: 10, discovered: 0, failure_count: 0, duration_ms: 5 });
      return { handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "workspace-r1", startedAt: "2026-01-01T00:00:00Z" }, unlisten: vi.fn() };
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be saved/i);
  });

  it("shows the scanner identity for progress events", async () => {
    vi.mocked(startScan).mockImplementation(async (_request, onEvent) => {
      onEvent({ kind: "progress", scanner_id: "windows.process", completed_units: 7, total_units: undefined, current_location: undefined });
      return { handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "workspace-r1", startedAt: "2026-01-01T00:00:00Z" }, unlisten: vi.fn() };
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    const progress = await screen.findByLabelText(/scan progress/i);
    expect(progress).toHaveTextContent("windows.process | 7 locations checked");
  });

  it("disables cancellation until the backend returns a scan id", async () => {
    vi.mocked(startScan).mockImplementation(() => new Promise(() => undefined));
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));

    expect(await screen.findByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("cleans up when backend cancellation fails", async () => {
    vi.mocked(startScan).mockResolvedValue({
      handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "workspace-r1", startedAt: "2026-01-01T00:00:00Z" },
      unlisten: vi.fn(),
    });
    vi.mocked(cancelScan).mockRejectedValue(new Error("already ended"));
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /run quick scan/i }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be confirmed/i);
    expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /run quick scan/i })).toBeEnabled();
  });
});
