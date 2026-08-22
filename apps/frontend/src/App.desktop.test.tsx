import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { bootstrap, cancelScan, pauseScan, pickScanRoots, resumeScan, reviewDiscovery, startScan } from "./api";

vi.mock("./api", () => ({
  bootstrap: vi.fn(),
  pickScanRoots: vi.fn(),
  startScan: vi.fn(),
  pauseScan: vi.fn(),
  resumeScan: vi.fn(),
  cancelScan: vi.fn(),
  reviewDiscovery: vi.fn(),
  isDesktop: vi.fn(() => true),
}));

beforeEach(() => {
  vi.mocked(bootstrap).mockReset();
  vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r1" });
  vi.mocked(startScan).mockReset();
  vi.mocked(pauseScan).mockReset();
  vi.mocked(resumeScan).mockReset();
  vi.mocked(cancelScan).mockReset();
  vi.mocked(pickScanRoots).mockReset();
  vi.mocked(reviewDiscovery).mockReset();
});
afterEach(cleanup);

describe("run scan dialog", () => {
  it("opens a dialog with Quick selected when Run scan is clicked", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));

    const dialog = await screen.findByRole("dialog", { name: "Run scan" });
    expect(within(dialog).getByRole("radio", { name: "Quick" })).toBeChecked();
    expect(within(dialog).getByRole("radio", { name: "Deep" })).not.toBeChecked();
  });

  it("reveals Select folders and the reparse checkbox unchecked when Deep is chosen", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));

    expect(screen.getByRole("button", { name: "Select folders" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Follow symbolic links and junctions" })).not.toBeChecked();
  });

  it("disables Run for Deep until at least one root is selected", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["C:\\Data"]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));

    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Select folders" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
  });

  it("shows selected roots only inside the dialog", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["C:\\Data"]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));

    expect(await screen.findByText("C:\\Data")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByText("C:\\Data")).not.toBeInTheDocument();
  });

  it("removes a selected root via its remove control without affecting the others", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["C:\\Data", "C:\\Tools"]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));

    expect(await screen.findByText("C:\\Data")).toBeInTheDocument();
    expect(screen.getByText("C:\\Tools")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove C:\\Data" }));

    expect(screen.queryByText("C:\\Data")).not.toBeInTheDocument();
    expect(screen.getByText("C:\\Tools")).toBeInTheDocument();
  });

  it("shows a description for each mode and a cloud-placeholder note for Deep", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    expect(screen.getByText(/built-in scanners/i)).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: "Deep" }));
    expect(screen.getByText(/reads only the folders you select/i)).toBeInTheDocument();
    expect(screen.getByText(/cloud-only files.*skipped, not downloaded/i)).toBeInTheDocument();
  });
});

describe("scan lifecycle controls", () => {
  async function beginQuickScanFlow(user: ReturnType<typeof userEvent.setup>, onEvent: (event: unknown) => void) {
    vi.mocked(startScan).mockImplementation(async (_request, handler) => {
      onEvent(handler);
      return {
        handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      };
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));
  }

  it("shows Paused after a paused event", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });

    emit({ kind: "paused" });

    expect(await screen.findByLabelText(/scan progress/i)).toHaveTextContent("Paused");
  });

  it("calls resumeScan with the current revision and stores the returned revision", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });
    emit({ kind: "paused" });
    await screen.findByRole("button", { name: "Resume" });

    vi.mocked(resumeScan).mockResolvedValue({ scanId: "scan-1", state: "running", revision: "scan-r2" });
    await user.click(screen.getByRole("button", { name: "Resume" }));

    expect(resumeScan).toHaveBeenCalledWith({ scanId: "scan-1", revision: "scan-r1" });

    vi.mocked(cancelScan).mockResolvedValue({ scanId: "scan-1", state: "cancelled", revision: "scan-r3" });
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(cancelScan).toHaveBeenCalledWith({ scanId: "scan-1", revision: "scan-r2" });
  });

  it("resyncs the per-scan revision from a pause failure's attached state instead of getting stuck", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });

    // pause_scan already rotated the revision in memory before persistence
    // failed, so the error carries the new state instead of an event.
    vi.mocked(pauseScan).mockRejectedValueOnce({
      code: "storage_integrity",
      message: "Local storage is unavailable",
      state: { scanId: "scan-1", state: "paused", revision: "scan-r2" },
    });
    await user.click(await screen.findByRole("button", { name: "Pause" }));

    // The bar picks up the new state/revision, and a subsequent mutation
    // uses the resynced revision rather than the stale pre-pause one.
    expect(await screen.findByLabelText(/scan progress/i)).toHaveTextContent("Paused");
    vi.mocked(resumeScan).mockResolvedValue({ scanId: "scan-1", state: "running", revision: "scan-r3" });
    await user.click(await screen.findByRole("button", { name: "Resume" }));
    expect(resumeScan).toHaveBeenCalledWith({ scanId: "scan-1", revision: "scan-r2" });
  });

  it("shows a scanner_failed warning without hiding prior discoveries", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });
    emit({ kind: "discovery", discovery: { id: "d1", suggested_name: "Found tool", suggested_type: "runtime", source_scanner: "s", confidence: "high", evidence: [], observed_at: "x", health_state: "unknown" } });
    emit({ kind: "scanner_failed", scanner_id: "windows.process", code: "io_error", message: "Could not read a location" });

    expect(await screen.findByRole("alert")).toHaveTextContent(/could not read a location/i);
    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();
  });

  it("streams a discovery into the Review Queue while the scan is still active, even while another scanner is slow/failing", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });

    // One scanner reports a discovery; another is still failing/slow. The
    // discovery must be visible in the Review Queue right away -- not only
    // after the scan reaches a terminal state.
    emit({
      kind: "discovery",
      discovery: {
        id: "d1",
        suggested_name: "Streamed Tool",
        suggested_type: "runtime",
        source_scanner: "fast.scanner",
        confidence: "high",
        evidence: [],
        observed_at: "x",
        health_state: "unknown",
      },
    });
    emit({ kind: "scanner_failed", scanner_id: "slow.scanner", code: "timeout", message: "Still waiting on a slow scanner" });

    // The scan bar still shows Running -- the scan has not terminated.
    expect(await screen.findByLabelText(/scan progress/i)).toHaveTextContent("Running");

    await user.click(screen.getByRole("button", { name: "Review Queue" }));
    expect(await screen.findByText("Streamed Tool")).toBeInTheDocument();
  });

  it("removes active controls on terminal but leaves a notice, and refreshes workspace revision", async () => {
    const user = userEvent.setup();
    let emit: (event: unknown) => void = () => undefined;
    await beginQuickScanFlow(user, (handler) => {
      emit = handler as (event: unknown) => void;
    });

    vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r2" });
    emit({ kind: "completed", visited: 3, discovered: 1, failure_count: 0, duration_ms: 10 });

    await waitFor(() => expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument());
    expect(await screen.findByText(/scan completed/i)).toBeInTheDocument();
    await waitFor(() => expect(bootstrap).toHaveBeenCalledTimes(2));

    vi.mocked(startScan).mockClear();
    vi.mocked(startScan).mockImplementation(
      () =>
        new Promise(() => {
          // never resolves; assert only on call args
        }),
    );
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));

    expect(startScan).toHaveBeenCalledWith(expect.objectContaining({ revision: "workspace-r2" }), expect.any(Function));
  });

  it("does not drop a progress event that arrives before start_scan resolves", async () => {
    const user = userEvent.setup();
    vi.mocked(startScan).mockImplementation(async (_request, handler) => {
      // Fire an event synchronously, before the handle promise settles —
      // matching the real listener-attached-before-invoke ordering.
      handler({ kind: "progress", scanner_id: "filesystem.deep", completed_units: 4, total_units: null, current_location: "Selected root 1 · depth 2" });
      await Promise.resolve();
      return {
        handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      };
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByLabelText(/scan progress/i)).toHaveTextContent("4");
  });

  it("treats a terminal event that arrives before start_scan resolves as already-terminal, not a stuck running scan", async () => {
    const user = userEvent.setup();
    vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r2" });
    vi.mocked(startScan).mockImplementation(async (_request, handler) => {
      handler({ kind: "failed", code: "scanner_failed", message: "The scan failed immediately", failure_count: 1, duration_ms: 1 });
      await Promise.resolve();
      return {
        handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      };
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));

    // The buffered terminal event must win over the "running" state set from
    // the resolved handle: no stuck scan bar, and the terminal notice shows.
    await waitFor(() => expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument());
    expect(await screen.findByText(/scan failed/i)).toBeInTheDocument();
  });
});

describe("start conflict recovery", () => {
  it("re-bootstraps and retries once with the fresh revision when start_scan returns conflict", async () => {
    const user = userEvent.setup();
    vi.mocked(startScan)
      .mockRejectedValueOnce({ code: "conflict" })
      .mockResolvedValueOnce({
        handle: { scanId: "scan-1", scope: "quick", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      });
    vi.mocked(bootstrap)
      .mockResolvedValueOnce({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r1" })
      .mockResolvedValueOnce({ mode: "desktop", pending: [], inventory: [], scanRevision: "workspace-r2" });

    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));
    expect(startScan).toHaveBeenNthCalledWith(1, expect.objectContaining({ revision: "workspace-r1" }), expect.any(Function));
    expect(startScan).toHaveBeenNthCalledWith(2, expect.objectContaining({ revision: "workspace-r2" }), expect.any(Function));
    expect(await screen.findByLabelText(/scan progress/i)).toBeInTheDocument();
  });
});

describe("network consent retry", () => {
  it("shows a confirmation naming network scanning and retries with networkConsent true", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["\\\\server\\share"]);
    vi.mocked(startScan)
      .mockRejectedValueOnce({ code: "network_consent_required" })
      .mockResolvedValueOnce({
        handle: { scanId: "scan-1", scope: "deep", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("One or more selected roots are on a network location. Allow this Deep Scan to read those network roots once?"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));
    const [firstRequest] = vi.mocked(startScan).mock.calls[0];
    const [secondRequest] = vi.mocked(startScan).mock.calls[1];
    expect(secondRequest).toEqual({ ...firstRequest, networkConsent: true });
  });

  it("retries with networkConsent true for a followed reparse-root rejection too, not just an obviously-UNC path", async () => {
    // The frontend never knows *why* the backend required consent (a
    // direct network root vs. a followed reparse root resolving to
    // network storage) -- it only reacts to the network_consent_required
    // code. This uses an ordinary local-looking selected path to prove the
    // confirmation/retry flow isn't accidentally keyed off the path's own
    // string shape.
    vi.mocked(pickScanRoots).mockResolvedValue(["C:\\Users\\me\\LinkedFolder"]);
    vi.mocked(startScan)
      .mockRejectedValueOnce({ code: "network_consent_required" })
      .mockResolvedValueOnce({
        handle: { scanId: "scan-1", scope: "deep", state: "running", revision: "scan-r1", startedAt: "2026-01-01T00:00:00Z" },
        unlisten: vi.fn(),
      });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("checkbox", { name: "Follow symbolic links and junctions" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Run" }));

    // start_scan rejected synchronously: no scan id/handle was ever
    // produced, so no active-scan controls appear -- only the consent
    // dialog, which the user can retry against.
    expect(await screen.findByRole("alertdialog", { name: "Network location confirmation" })).toBeInTheDocument();
    expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));
    const [secondRequest] = vi.mocked(startScan).mock.calls[1];
    expect(secondRequest).toMatchObject({ networkConsent: true, followReparsePoints: true });
    expect(await screen.findByLabelText(/scan progress/i)).toBeInTheDocument();
  });

  it("cancelling the network-consent confirmation leaves scan state unchanged (no retry, no active scan)", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["\\\\server\\share"]);
    vi.mocked(startScan).mockRejectedValue({ code: "network_consent_required" });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Run" }));

    await screen.findByRole("alertdialog", { name: "Network location confirmation" });
    expect(startScan).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Cancel" }));

    // No retry was ever issued, and there is no active scan to show.
    expect(startScan).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alertdialog", { name: "Network location confirmation" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument();
  });

  it("resets networkConsent to false when the dialog is closed and reopened", async () => {
    vi.mocked(pickScanRoots).mockResolvedValue(["\\\\server\\share"]);
    vi.mocked(startScan).mockRejectedValue({ code: "network_consent_required" });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText(/network location/i);
    await user.click(screen.getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));

    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("radio", { name: "Deep" }));
    await user.click(screen.getByRole("button", { name: "Select folders" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    vi.mocked(startScan).mockClear();
    vi.mocked(startScan).mockRejectedValue({ code: "network_consent_required" });
    await user.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(1));
    const [request] = vi.mocked(startScan).mock.calls[0];
    expect(request).toMatchObject({ networkConsent: false });
  });
});
