import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { bootstrap, cancelHealthRun, cancelScan, listenHealthEvents, listHealthSummary, listInstallationHealth, pauseScan, pickScanRoots, resumeScan, reviewDiscovery, runAllHealthChecks, runHealthChecks, startScan } from "./api";

vi.mock("./api", () => ({
  bootstrap: vi.fn(),
  pickScanRoots: vi.fn(),
  startScan: vi.fn(),
  pauseScan: vi.fn(),
  resumeScan: vi.fn(),
  cancelScan: vi.fn(),
  reviewDiscovery: vi.fn(),
  runHealthChecks: vi.fn(),
  runAllHealthChecks: vi.fn(),
  cancelHealthRun: vi.fn(),
  listHealthSummary: vi.fn(),
  listInstallationHealth: vi.fn(),
  listenHealthEvents: vi.fn(),
  isDesktop: vi.fn(() => true),
}));

beforeEach(() => {
  vi.mocked(bootstrap).mockReset();
  vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], capabilities: [], installations: [], scanRevision: "workspace-r1", healthRevision: "health-r1", dependencyRevision: "dependency-r1" });
  vi.mocked(startScan).mockReset();
  vi.mocked(pauseScan).mockReset();
  vi.mocked(resumeScan).mockReset();
  vi.mocked(cancelScan).mockReset();
  vi.mocked(pickScanRoots).mockReset();
  vi.mocked(reviewDiscovery).mockReset();
  vi.mocked(runHealthChecks).mockReset();
  vi.mocked(runAllHealthChecks).mockReset();
  vi.mocked(cancelHealthRun).mockReset();
  vi.mocked(listHealthSummary).mockReset();
  vi.mocked(listHealthSummary).mockRejectedValue(
    new Error("health summary unavailable unless configured by this test"),
  );
  vi.mocked(listInstallationHealth).mockReset();
  vi.mocked(listenHealthEvents).mockReset();
  vi.mocked(listenHealthEvents).mockResolvedValue(() => {});
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

    vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], capabilities: [], installations: [], scanRevision: "workspace-r2", healthRevision: "health-r2", dependencyRevision: "dependency-r2" });
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
    vi.mocked(bootstrap).mockResolvedValue({ mode: "desktop", pending: [], capabilities: [], installations: [], scanRevision: "workspace-r2", healthRevision: "health-r2", dependencyRevision: "dependency-r2" });
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
      .mockResolvedValueOnce({ mode: "desktop", pending: [], capabilities: [], installations: [], scanRevision: "workspace-r1", healthRevision: "health-r1", dependencyRevision: "dependency-r1" })
      .mockResolvedValueOnce({ mode: "desktop", pending: [], capabilities: [], installations: [], scanRevision: "workspace-r2", healthRevision: "health-r2", dependencyRevision: "dependency-r2" });

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

describe("health workspace UI", () => {
  it("health_ui_lists_reviewed_installations_not_pending_discoveries", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [
        {
          id: "disc-pending",
          suggested_name: "Pending Runtime",
          suggested_type: "runtime",
          source_scanner: "test",
          confidence: "high",
          evidence: [{ kind: "test", summary: "Pending evidence" }],
          observed_at: "2026-01-01T00:00:00Z",
          health_state: "unknown",
        },
      ],
      capabilities: [
        {
          id: "cap-reviewed",
          canonicalName: "Reviewed Capability",
          stableIdentity: "runtime:reviewed",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-reviewed",
          capabilityId: "cap-reviewed",
          sourceDiscoveryId: "disc-reviewed",
          kind: "runtime",
          displayName: "Reviewed Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));

    expect(await screen.findByText("Reviewed Runtime")).toBeInTheDocument();
    expect(screen.queryByText("Pending Runtime")).not.toBeInTheDocument();
  });
  it("health_ui_check_all_invokes_explicit_run", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-one",
          canonicalName: "Capability One",
          stableIdentity: "runtime:one",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-one",
          capabilityId: "cap-one",
          sourceDiscoveryId: "disc-one",
          kind: "runtime",
          displayName: "Runtime One",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(runAllHealthChecks).mockResolvedValue({
      runId: "run-all-1",
      scope: "all",
      state: "running",
      revision: "health-r2",
      startedAt: "2026-01-01T00:00:01Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    expect(runAllHealthChecks).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Check All" }));

    await waitFor(() => expect(runAllHealthChecks).toHaveBeenCalledTimes(1));
  });

  it("health_ui_single_run_invokes_selected_installation", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-one",
          canonicalName: "Capability One",
          stableIdentity: "runtime:one",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
        {
          id: "cap-two",
          canonicalName: "Capability Two",
          stableIdentity: "runtime:two",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-one",
          capabilityId: "cap-one",
          sourceDiscoveryId: "disc-one",
          kind: "runtime",
          displayName: "Runtime One",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
        {
          id: "inst-two",
          capabilityId: "cap-two",
          sourceDiscoveryId: "disc-two",
          kind: "runtime",
          displayName: "Runtime Two",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(runHealthChecks).mockResolvedValue({
      runId: "run-one",
      scope: "installation",
      state: "running",
      revision: "health-r2",
      startedAt: "2026-01-01T00:00:01Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    const runtimeTwo = screen.getByText("Runtime Two").closest("article");
    if (!runtimeTwo) throw new Error("Runtime Two row not found");

    await user.click(within(runtimeTwo).getByRole("button", { name: "Run checks" }));

    await waitFor(() => expect(runHealthChecks).toHaveBeenCalledTimes(1));
    expect(vi.mocked(runHealthChecks).mock.calls[0][0]).toBe("inst-two");
    expect(runAllHealthChecks).not.toHaveBeenCalled();
  });
  it("health_ui_active_check_all_shows_requested_scheduled_and_skipped_counts", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-active",
          canonicalName: "Active Capability",
          stableIdentity: "runtime:active",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-active",
          capabilityId: "cap-active",
          sourceDiscoveryId: "disc-active",
          kind: "runtime",
          displayName: "Active Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(runAllHealthChecks).mockResolvedValue({
      runId: "run-all-active",
      scope: "all",
      state: "running",
      revision: "health-r2",
      startedAt: "2026-01-01T00:00:01Z",
      requestedInstallationCount: 5,
      scheduledInstallationCount: 3,
      skippedActiveInstallationCount: 2,
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByRole("button", { name: "Check All" }));

    expect(await screen.findByText(/Requested 5/i)).toBeInTheDocument();
    expect(screen.getByText(/Scheduled 3/i)).toBeInTheDocument();
    expect(screen.getByText(/Skipped 2/i)).toBeInTheDocument();
    expect(screen.queryByText(/Completed 3/i)).not.toBeInTheDocument();
  });

  it("health_ui_cancel_control_cancels_active_run", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [],
      installations: [],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(runAllHealthChecks).mockResolvedValue({
      runId: "run-cancel-me",
      scope: "all",
      state: "running",
      revision: "health-r2",
      startedAt: "2026-01-01T00:00:01Z",
      requestedInstallationCount: 0,
      scheduledInstallationCount: 0,
      skippedActiveInstallationCount: 0,
    });

    vi.mocked(cancelHealthRun).mockResolvedValue({
      runId: "run-cancel-me",
      state: "cancelling",
      revision: "health-r3",
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByRole("button", { name: "Check All" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(cancelHealthRun).toHaveBeenCalledTimes(1));
    expect(vi.mocked(cancelHealthRun).mock.calls[0][0]).toBe("run-cancel-me");
  });

  it("health_ui_never_checked_installation_displays_not_checked", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-never",
          canonicalName: "Never Checked Capability",
          stableIdentity: "runtime:never",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-never",
          capabilityId: "cap-never",
          sourceDiscoveryId: "disc-never",
          kind: "runtime",
          displayName: "Never Checked Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      summary: {
        capabilityCount: 1,
        installationCount: 1,
        pendingDiscoveryCount: 0,
        greenCount: 0,
        yellowCount: 0,
        redCount: 0,
        greyCount: 1,
        blueCount: 0,
      },
      installations: [
        {
          installation: {
            id: "inst-never",
            capabilityId: "cap-never",
            sourceDiscoveryId: "disc-never",
            kind: "runtime",
            displayName: "Never Checked Runtime",
            host: "localhost",
            installationState: "installed",
            registrationState: "registered",
            enablementState: "enabled",
            runtimeState: "unknown",
            connectionState: "unknown",
            authenticationState: "unknown",
            aggregateHealth: "grey",
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
          },
          capability: {
            id: "cap-never",
            canonicalName: "Never Checked Capability",
            stableIdentity: "runtime:never",
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
            revision: "cap-r1",
          },
          checks: [],
          effectiveHealth: "grey",
          lastCheckedAt: null,
          warningCount: 0,
          failureCount: 0,
        },
      ],
      revision: "health-r2",
    });

    const user = userEvent.setup();
    render(<App />);

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "Health" }));

    const row = screen.getByText("Never Checked Runtime").closest("article");
    if (!row) throw new Error("Never Checked Runtime row not found");

    expect(within(row).getByText("Not checked")).toBeInTheDocument();
    expect(within(row).getByText(/Last checked:\s*Not checked/i)).toBeInTheDocument();
    expect(within(row).getByText(/Warnings:\s*0/i)).toBeInTheDocument();
    expect(within(row).getByText(/Failures:\s*0/i)).toBeInTheDocument();
  });
  it.each([
    [
      "health_ui_blue_is_setup_required_not_failure",
      "blue",
      "Setup required",
      "Failed required check",
    ],
    [
      "health_ui_yellow_is_warning",
      "yellow",
      "Warning",
      "Healthy",
    ],
    [
      "health_ui_red_is_required_failure",
      "red",
      "Failed required check",
      "Warning",
    ],
  ] as const)(
    "%s",
    async (_name, healthState, expectedLabel, absentLabel) => {
      vi.mocked(bootstrap).mockResolvedValueOnce({
        mode: "desktop",
        pending: [],
        capabilities: [
          {
            id: `cap-${healthState}`,
            canonicalName: `${expectedLabel} Capability`,
            stableIdentity: `runtime:${healthState}`,
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
            revision: "cap-r1",
          },
        ],
        installations: [
          {
            id: `inst-${healthState}`,
            capabilityId: `cap-${healthState}`,
            sourceDiscoveryId: `disc-${healthState}`,
            kind: "runtime",
            displayName: `${expectedLabel} Runtime`,
            host: "localhost",
            installationState: "installed",
            registrationState: "registered",
            enablementState: "enabled",
            runtimeState: "unknown",
            connectionState: "unknown",
            authenticationState: "unknown",
            aggregateHealth: healthState,
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
          },
        ],
        scanRevision: "scan-r1",
        healthRevision: "health-r1",
        dependencyRevision: "dependency-r1",
      } as never);

      vi.mocked(listHealthSummary).mockResolvedValueOnce({
        summary: {
          capabilityCount: 1,
          installationCount: 1,
          pendingDiscoveryCount: 0,
          greenCount: 0,
          yellowCount: healthState === "yellow" ? 1 : 0,
          redCount: healthState === "red" ? 1 : 0,
          greyCount: 0,
          blueCount: healthState === "blue" ? 1 : 0,
        },
        installations: [
          {
            installation: {
              id: `inst-${healthState}`,
              capabilityId: `cap-${healthState}`,
              sourceDiscoveryId: `disc-${healthState}`,
              kind: "runtime",
              displayName: `${expectedLabel} Runtime`,
              host: "localhost",
              installationState: "installed",
              registrationState: "registered",
              enablementState: "enabled",
              runtimeState: "unknown",
              connectionState: "unknown",
              authenticationState: "unknown",
              aggregateHealth: healthState,
              createdAt: "2026-01-01T00:00:00Z",
              updatedAt: "2026-01-01T00:00:00Z",
            },
            capability: {
              id: `cap-${healthState}`,
              canonicalName: `${expectedLabel} Capability`,
              stableIdentity: `runtime:${healthState}`,
              createdAt: "2026-01-01T00:00:00Z",
              updatedAt: "2026-01-01T00:00:00Z",
              revision: "cap-r1",
            },
            checks: [],
            effectiveHealth: healthState,
            lastCheckedAt: "2026-01-01T00:00:05Z",
            warningCount: healthState === "yellow" ? 1 : 0,
            failureCount: healthState === "red" ? 1 : 0,
          },
        ],
        revision: "health-r2",
      });

      const user = userEvent.setup();
      render(<App />);

      await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(1));
      await user.click(screen.getByRole("button", { name: "Health" }));

      const row = screen
        .getByText(`${expectedLabel} Runtime`, { selector: "strong" })
        .closest("article");
      if (!row) throw new Error(`${expectedLabel} row not found`);

      expect(within(row).getByText(expectedLabel)).toBeInTheDocument();
      expect(within(row).queryByText(absentLabel)).not.toBeInTheDocument();
    },
  );
  it("health_ui_checking_is_temporary_not_persisted_health", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-warning",
          canonicalName: "Warning Capability",
          stableIdentity: "runtime:warning",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-warning",
          capabilityId: "cap-warning",
          sourceDiscoveryId: "disc-warning",
          kind: "runtime",
          displayName: "Warning Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "yellow",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(runAllHealthChecks).mockResolvedValue({
      runId: "run-warning",
      scope: "all",
      state: "running",
      revision: "health-r2",
      startedAt: "2026-01-01T00:00:01Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByRole("button", { name: "Check All" }));

    expect(await screen.findByText("Checking")).toBeInTheDocument();
    expect(screen.getByText("Warning", { selector: "b" })).toBeInTheDocument();
    expect(screen.queryByText("Healthy")).not.toBeInTheDocument();
  });
});
describe("health installation detail", () => {
  it("health_ui_detail_loads_on_selection", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-detail",
          canonicalName: "Detail Capability",
          stableIdentity: "runtime:detail",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-detail",
          capabilityId: "cap-detail",
          sourceDiscoveryId: "disc-detail",
          kind: "runtime",
          displayName: "Detail Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "yellow",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listInstallationHealth).mockResolvedValue({
      snapshot: {
        installation: {
          id: "inst-detail",
          capabilityId: "cap-detail",
          sourceDiscoveryId: "disc-detail",
          kind: "runtime",
          displayName: "Detail Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "yellow",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
        capability: {
          id: "cap-detail",
          canonicalName: "Detail Capability",
          stableIdentity: "runtime:detail",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
        checks: [],
        effectiveHealth: "yellow",
        lastCheckedAt: "2026-01-01T00:00:05Z",
        warningCount: 1,
        failureCount: 0,
      },
      history: [],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByText("Detail Runtime"));

    await waitFor(() =>
      expect(listInstallationHealth).toHaveBeenCalledWith("inst-detail"),
    );
    expect(
      await screen.findByRole("heading", { name: "Detail Runtime" }),
    ).toBeInTheDocument();
    const detail = screen.getByRole("region", { name: "Installation health detail" });
    expect(within(detail).getByText("Detail Capability")).toBeInTheDocument();
  });
});
describe("health installation detail contracts", () => {
  it.each([
    "health_ui_detail_shows_required_optional_state_duration_and_time",
    "health_ui_detail_marks_stale_checks",
    "health_ui_detail_shows_only_redacted_evidence_field",
    "health_ui_history_is_newest_first",
  ])("%s", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-checks",
          canonicalName: "Checks Capability",
          stableIdentity: "runtime:checks",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-checks",
          capabilityId: "cap-checks",
          sourceDiscoveryId: "disc-checks",
          kind: "runtime",
          displayName: "Checks Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "yellow",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listInstallationHealth).mockResolvedValue({
      snapshot: {
        installation: {
          id: "inst-checks",
          capabilityId: "cap-checks",
          sourceDiscoveryId: "disc-checks",
          kind: "runtime",
          displayName: "Checks Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "yellow",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
        capability: {
          id: "cap-checks",
          canonicalName: "Checks Capability",
          stableIdentity: "runtime:checks",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
        checks: [
          {
            definition: {
              id: "check-required",
              installationId: "inst-checks",
              checkType: "config_parses",
              required: true,
              expectedCondition: "parses",
              timeoutMs: 1000,
              staleAfterSeconds: 60,
              target: {
                kind: "config",
                path: "C:\\secret\\config.json",
                format: "json",
              },
              createdAt: "2026-01-01T00:00:00Z",
              updatedAt: "2026-01-01T00:00:00Z",
            },
            latestResult: {
              id: "result-required",
              runId: "run-detail",
              installationId: "inst-checks",
              checkDefinitionId: "check-required",
              checkType: "config_parses",
              required: true,
              state: "yellow",
              resultCode: "config_stale",
              redactedEvidence: "Configuration parsed from an approved local source.",
              durationMs: 42,
              observedAt: "2026-01-01T00:00:05Z",
            },
            stale: true,
          },
          {
            definition: {
              id: "check-optional",
              installationId: "inst-checks",
              checkType: "process_running",
              required: false,
              expectedCondition: "running",
              timeoutMs: 1000,
              staleAfterSeconds: null,
              target: {
                kind: "process_executable",
                path: "C:\\secret\\runtime.exe",
              },
              createdAt: "2026-01-01T00:00:00Z",
              updatedAt: "2026-01-01T00:00:00Z",
            },
            latestResult: {
              id: "result-optional",
              runId: "run-detail",
              installationId: "inst-checks",
              checkDefinitionId: "check-optional",
              checkType: "process_running",
              required: false,
              state: "green",
              resultCode: "process_running",
              redactedEvidence: "Process is running.",
              durationMs: 7,
              observedAt: "2026-01-01T00:00:06Z",
            },
            stale: false,
          },
        ],
        effectiveHealth: "yellow",
        lastCheckedAt: "2026-01-01T00:00:06Z",
        warningCount: 1,
        failureCount: 0,
      },
      history: [
        {
          id: "history-new",
          runId: "run-new",
          installationId: "inst-checks",
          checkDefinitionId: "check-required",
          checkType: "config_parses",
          required: true,
          state: "yellow",
          resultCode: "newest-result",
          redactedEvidence: "Newest safe evidence.",
          durationMs: 31,
          observedAt: "2026-01-02T00:00:00Z",
        },
        {
          id: "history-old",
          runId: "run-old",
          installationId: "inst-checks",
          checkDefinitionId: "check-required",
          checkType: "config_parses",
          required: true,
          state: "green",
          resultCode: "older-result",
          redactedEvidence: "Older safe evidence.",
          durationMs: 21,
          observedAt: "2026-01-01T00:00:00Z",
        },
      ],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByRole("button", { name: /Checks Runtime/i }));

    const detail = await screen.findByRole("region", {
      name: "Installation health detail",
    });

    expect(within(detail).getByText("Required")).toBeInTheDocument();
    expect(within(detail).getByText("Optional")).toBeInTheDocument();
    expect(within(detail).getByText("Warning")).toBeInTheDocument();
    expect(within(detail).getByText("Healthy")).toBeInTheDocument();
    expect(within(detail).getByText(/Result code:\s*config_stale/i)).toBeInTheDocument();
    expect(within(detail).getByText(/Duration:\s*42\s*ms/i)).toBeInTheDocument();
    expect(within(detail).getByText(/Observed:\s*2026-01-01T00:00:05Z/i)).toBeInTheDocument();
    expect(within(detail).getByText("Stale")).toBeInTheDocument();
    expect(
      within(detail).getByText(
        "Configuration parsed from an approved local source.",
      ),
    ).toBeInTheDocument();

    expect(
      within(detail).queryByText("C:\\secret\\config.json"),
    ).not.toBeInTheDocument();
    expect(
      within(detail).queryByText("C:\\secret\\runtime.exe"),
    ).not.toBeInTheDocument();

    const newest = within(detail).getByText("newest-result");
    const older = within(detail).getByText("older-result");
    expect(
      newest.compareDocumentPosition(older) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("health_ui_no_definitions_remains_grey", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-none",
          canonicalName: "No Checks Capability",
          stableIdentity: "runtime:no-checks",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-none",
          capabilityId: "cap-none",
          sourceDiscoveryId: "disc-none",
          kind: "runtime",
          displayName: "No Checks Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listInstallationHealth).mockResolvedValue({
      snapshot: {
        installation: {
          id: "inst-none",
          capabilityId: "cap-none",
          sourceDiscoveryId: "disc-none",
          kind: "runtime",
          displayName: "No Checks Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
        capability: {
          id: "cap-none",
          canonicalName: "No Checks Capability",
          stableIdentity: "runtime:no-checks",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
        checks: [],
        effectiveHealth: "grey",
        lastCheckedAt: null,
        warningCount: 0,
        failureCount: 0,
      },
      history: [],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Health" }));
    await user.click(screen.getByRole("button", { name: /No Checks Runtime/i }));

    const detail = await screen.findByRole("region", {
      name: "Installation health detail",
    });

    expect(
      within(detail).getByText(/no supported local health checks are configured/i),
    ).toBeInTheDocument();
    expect(within(detail).getByText(/Current state: Not checked/i)).toBeInTheDocument();
    expect(within(detail).queryByText("Healthy")).not.toBeInTheDocument();
  });
});
describe("health inventory and overview integration", () => {
  it("health_ui_inventory_uses_installation_instances", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-instance",
          canonicalName: "Canonical Runtime",
          stableIdentity: "runtime:canonical",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-instance",
          capabilityId: "cap-instance",
          sourceDiscoveryId: "disc-instance",
          kind: "runtime",
          displayName: "Laptop Runtime Instance",
          host: "workstation-a",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Inventory" }));

    const instanceName = await screen.findByText("Laptop Runtime Instance");
    const row = instanceName.closest("article");
    if (!row) throw new Error("Inventory installation row not found");

    expect(within(row).getByText("Canonical Runtime")).toBeInTheDocument();
    expect(
      within(row).getByText(/runtime.*workstation-a/i, { selector: "em" }),
    ).toBeInTheDocument();
    expect(within(row).getByText("Not checked")).toBeInTheDocument();
  });
  it("health_ui_inventory_prefers_effective_health_projection", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-inventory",
          canonicalName: "Inventory Capability",
          stableIdentity: "runtime:inventory",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-inventory",
          capabilityId: "cap-inventory",
          sourceDiscoveryId: "disc-inventory",
          kind: "runtime",
          displayName: "Inventory Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "green",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      summary: {
        capabilityCount: 1,
        installationCount: 1,
        pendingDiscoveryCount: 0,
        greenCount: 0,
        yellowCount: 1,
        redCount: 0,
        greyCount: 0,
        blueCount: 0,
      },
      installations: [
        {
          installation: {
            id: "inst-inventory",
            capabilityId: "cap-inventory",
            sourceDiscoveryId: "disc-inventory",
            kind: "runtime",
            displayName: "Inventory Runtime",
            host: "localhost",
            installationState: "installed",
            registrationState: "registered",
            enablementState: "enabled",
            runtimeState: "unknown",
            connectionState: "unknown",
            authenticationState: "unknown",
            aggregateHealth: "green",
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
          },
          capability: {
            id: "cap-inventory",
            canonicalName: "Inventory Capability",
            stableIdentity: "runtime:inventory",
            createdAt: "2026-01-01T00:00:00Z",
            updatedAt: "2026-01-01T00:00:00Z",
            revision: "cap-r1",
          },
          checks: [],
          effectiveHealth: "yellow",
          lastCheckedAt: "2026-01-01T00:00:05Z",
          warningCount: 1,
          failureCount: 0,
        },
      ],
      revision: "health-r2",
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Inventory" }));

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalled());
    expect(await screen.findByText("Inventory Capability")).toBeInTheDocument();
    expect(screen.getByText("Warning")).toBeInTheDocument();
    expect(screen.queryByText("Healthy")).not.toBeInTheDocument();
  });

  it.each([
    "health_ui_overview_uses_backend_health_counts",
    "health_ui_overview_counts_installations_separately_from_capabilities",
    "health_ui_overview_health_counts_total_installations",
    "health_ui_overview_removes_hardcoded_zero_failures",
    "health_ui_read_only_refresh_may_run_on_navigation_but_health_checks_do_not",
  ])("%s", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [
        {
          id: "disc-pending",
          suggested_name: "Pending Runtime",
          suggested_type: "runtime",
          source_scanner: "test",
          confidence: "high",
          evidence: [{ kind: "test", summary: "Pending evidence" }],
          observed_at: "2026-01-01T00:00:00Z",
          health_state: "unknown",
        },
      ],
      capabilities: [
        {
          id: "cap-a",
          canonicalName: "Capability A",
          stableIdentity: "runtime:a",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
        {
          id: "cap-b",
          canonicalName: "Capability B",
          stableIdentity: "runtime:b",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      summary: {
        capabilityCount: 2,
        installationCount: 5,
        pendingDiscoveryCount: 1,
        greenCount: 1,
        yellowCount: 1,
        redCount: 1,
        greyCount: 1,
        blueCount: 1,
      },
      installations: [],
      revision: "health-r2",
    });

    render(<App />);

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(1));

    const metrics = document.querySelector<HTMLElement>(".metrics");
    if (!metrics) throw new Error("metrics section not found");

    const metric = (label: string) => {
      const labelNode = within(metrics).getByText(label);
      const article = labelNode.closest("article");
      if (!article) throw new Error(`${label} metric not found`);
      return within(article);
    };

    expect(metric("Capabilities").getByText("02")).toBeInTheDocument();
    expect(metric("Installations").getByText("05")).toBeInTheDocument();
    expect(metric("Pending review").getByText("01")).toBeInTheDocument();
    expect(metric("Healthy").getByText("01")).toBeInTheDocument();
    expect(metric("Warnings").getByText("01")).toBeInTheDocument();
    expect(metric("Failures").getByText("01")).toBeInTheDocument();
    expect(metric("Unknown").getByText("01")).toBeInTheDocument();
    expect(metric("Setup required").getByText("01")).toBeInTheDocument();

    expect(runHealthChecks).not.toHaveBeenCalled();
    expect(runAllHealthChecks).not.toHaveBeenCalled();
  });
  it("health_ui_overview_pending_is_not_unknown_health", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [
        {
          id: "disc-pending-a",
          suggested_name: "Pending A",
          suggested_type: "runtime",
          source_scanner: "test",
          confidence: "high",
          evidence: [{ kind: "test", summary: "Pending A evidence" }],
          observed_at: "2026-01-01T00:00:00Z",
          health_state: "unknown",
        },
        {
          id: "disc-pending-b",
          suggested_name: "Pending B",
          suggested_type: "runtime",
          source_scanner: "test",
          confidence: "high",
          evidence: [{ kind: "test", summary: "Pending B evidence" }],
          observed_at: "2026-01-01T00:00:01Z",
          health_state: "unknown",
        },
        {
          id: "disc-pending-c",
          suggested_name: "Pending C",
          suggested_type: "runtime",
          source_scanner: "test",
          confidence: "high",
          evidence: [{ kind: "test", summary: "Pending C evidence" }],
          observed_at: "2026-01-01T00:00:02Z",
          health_state: "unknown",
        },
      ],
      capabilities: [
        {
          id: "cap-grey-overview",
          canonicalName: "Grey Overview Capability",
          stableIdentity: "runtime:grey-overview",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-grey-overview",
          capabilityId: "cap-grey-overview",
          sourceDiscoveryId: "disc-reviewed",
          kind: "runtime",
          displayName: "Grey Reviewed Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      summary: {
        capabilityCount: 1,
        installationCount: 1,
        pendingDiscoveryCount: 3,
        greenCount: 0,
        yellowCount: 0,
        redCount: 0,
        greyCount: 1,
        blueCount: 0,
      },
      installations: [],
      revision: "health-r2",
    });

    render(<App />);

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(1));

    const metrics = document.querySelector<HTMLElement>(".metrics");
    if (!metrics) throw new Error("metrics section not found");

    const metric = (label: string) => {
      const labelNode = within(metrics).getByText(label);
      const article = labelNode.closest("article");
      if (!article) throw new Error(`${label} metric not found`);
      return within(article);
    };

    expect(metric("Pending review").getByText("03")).toBeInTheDocument();
    expect(metric("Unknown").getByText("01")).toBeInTheDocument();
  });
});
  it("health_ui_refreshes_authoritative_summary_after_health_revision_changes", async () => {
    vi.mocked(bootstrap)
      .mockResolvedValueOnce({
        mode: "desktop",
        pending: [
          {
            id: "disc-refresh",
            suggested_name: "Refresh Runtime",
            suggested_type: "runtime",
            source_scanner: "test",
            confidence: "high",
            evidence: [],
            observed_at: "2026-01-01T00:00:00Z",
            health_state: "unknown",
          },
        ],
        capabilities: [],
        installations: [],
        scanRevision: "scan-r1",
        healthRevision: "health-r1",
        dependencyRevision: "dependency-r1",
      } as never)
      .mockResolvedValueOnce({
        mode: "desktop",
        pending: [],
        capabilities: [
          {
            id: "cap-refresh",
            canonicalName: "Refresh Capability",
            stableIdentity: "runtime:refresh",
            createdAt: "2026-01-01T00:00:01Z",
            updatedAt: "2026-01-01T00:00:01Z",
            revision: "cap-r1",
          },
        ],
        installations: [
          {
            id: "inst-refresh",
            capabilityId: "cap-refresh",
            sourceDiscoveryId: "disc-refresh",
            kind: "runtime",
            displayName: "Refresh Runtime",
            host: "localhost",
            installationState: "installed",
            registrationState: "registered",
            enablementState: "enabled",
            runtimeState: "unknown",
            connectionState: "unknown",
            authenticationState: "unknown",
            aggregateHealth: "grey",
            createdAt: "2026-01-01T00:00:01Z",
            updatedAt: "2026-01-01T00:00:01Z",
          },
        ],
        scanRevision: "scan-r2",
        healthRevision: "health-r2",
        dependencyRevision: "dependency-r1",
      } as never);

    vi.mocked(reviewDiscovery).mockResolvedValue(undefined as never);

    vi.mocked(listHealthSummary)
      .mockResolvedValueOnce({
        summary: {
          capabilityCount: 0,
          installationCount: 0,
          pendingDiscoveryCount: 1,
          greenCount: 0,
          yellowCount: 0,
          redCount: 0,
          greyCount: 0,
          blueCount: 0,
        },
        installations: [],
        revision: "health-r1",
      })
      .mockResolvedValueOnce({
        summary: {
          capabilityCount: 1,
          installationCount: 1,
          pendingDiscoveryCount: 0,
          greenCount: 0,
          yellowCount: 0,
          redCount: 0,
          greyCount: 1,
          blueCount: 0,
        },
        installations: [
          {
            installation: {
              id: "inst-refresh",
              capabilityId: "cap-refresh",
              sourceDiscoveryId: "disc-refresh",
              kind: "runtime",
              displayName: "Refresh Runtime",
              host: "localhost",
              installationState: "installed",
              registrationState: "registered",
              enablementState: "enabled",
              runtimeState: "unknown",
              connectionState: "unknown",
              authenticationState: "unknown",
              aggregateHealth: "grey",
              createdAt: "2026-01-01T00:00:01Z",
              updatedAt: "2026-01-01T00:00:01Z",
            },
            capability: {
              id: "cap-refresh",
              canonicalName: "Refresh Capability",
              stableIdentity: "runtime:refresh",
              createdAt: "2026-01-01T00:00:01Z",
              updatedAt: "2026-01-01T00:00:01Z",
              revision: "cap-r1",
            },
            checks: [],
            effectiveHealth: "grey",
            lastCheckedAt: null,
            warningCount: 0,
            failureCount: 0,
          },
        ],
        revision: "health-r2",
      });

    const user = userEvent.setup();
    render(<App />);

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("button", { name: "Review Queue" }));
    const reviewItem = await screen.findByText("Refresh Runtime");
    const review = reviewItem.closest(".review") as HTMLElement | null;
    if (!review) throw new Error("Refresh Runtime review card not found");

    await user.click(within(review).getByRole("button", { name: "Import" }));

    await waitFor(() => expect(bootstrap).toHaveBeenCalledTimes(2));

    await user.click(screen.getByRole("button", { name: "Health" }));

    await waitFor(() => expect(listHealthSummary).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Refresh Capability")).toBeInTheDocument();
    expect(screen.getByText("Refresh Runtime")).toBeInTheDocument();
  });
describe("health frontend bootstrap contracts", () => {
  it("health_frontend_navigation_does_not_start_checks", async () => {
    const user = userEvent.setup();

    render(<App />);
    await screen.findByText("Capabilities");

    await user.click(screen.getByRole("button", { name: "Health" }));
    expect(screen.getByRole("button", { name: "Health" })).toHaveClass("active");

    await user.click(screen.getByRole("button", { name: "Inventory" }));
    expect(screen.getByRole("button", { name: "Inventory" })).toHaveClass("active");

    await user.click(screen.getByRole("button", { name: "Overview" }));
    expect(screen.getByRole("button", { name: "Overview" })).toHaveClass("active");

    expect(runHealthChecks).not.toHaveBeenCalled();
    expect(runAllHealthChecks).not.toHaveBeenCalled();
  });
  it("health_ui_inventory_bootstrap_grey_is_not_promoted", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-grey",
          canonicalName: "Unchecked Runtime",
          stableIdentity: "runtime:unchecked",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-grey-r1",
        },
      ],
      installations: [
        {
          id: "inst-grey",
          capabilityId: "cap-grey",
          sourceDiscoveryId: "disc-grey",
          kind: "runtime",
          displayName: "Unchecked Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Inventory" }));

    expect(await screen.findByText("Unchecked Runtime", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByText("Not checked")).toBeInTheDocument();
    expect(screen.queryByText(/^green$/i)).not.toBeInTheDocument();
  });
  it("health_frontend_bootstrap_uses_capabilities_installations_and_independent_revisions", async () => {
    vi.mocked(bootstrap).mockResolvedValueOnce({
      mode: "desktop",
      pending: [],
      capabilities: [
        {
          id: "cap-1",
          canonicalName: "Example Runtime",
          stableIdentity: "runtime:example",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
          revision: "cap-r1",
        },
      ],
      installations: [
        {
          id: "inst-1",
          capabilityId: "cap-1",
          sourceDiscoveryId: "disc-1",
          kind: "runtime",
          displayName: "Example Runtime",
          host: "localhost",
          installationState: "installed",
          registrationState: "registered",
          enablementState: "enabled",
          runtimeState: "unknown",
          connectionState: "unknown",
          authenticationState: "unknown",
          aggregateHealth: "grey",
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
      scanRevision: "scan-r1",
      healthRevision: "health-r1",
      dependencyRevision: "dependency-r1",
    } as never);

    render(<App />);

    await screen.findByText("Capabilities");

    const metrics = document.querySelector<HTMLElement>(".metrics");
    if (!metrics) throw new Error("metrics section not found");

    const metric = (label: string) => {
      const labelNode = within(metrics).getByText(label);
      const article = labelNode.closest("article");
      if (!article) throw new Error(`${label} metric not found`);
      return within(article);
    };

    expect(metric("Capabilities").getByText("01")).toBeInTheDocument();
    expect(metric("Installations").getByText("01")).toBeInTheDocument();
    expect(metric("Unknown").getByText("01")).toBeInTheDocument();
  });
});
