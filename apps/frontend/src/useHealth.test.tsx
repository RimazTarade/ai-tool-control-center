import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHealth } from "./useHealth";
import {
  cancelHealthRun,
  listHealthSummary,
  listInstallationHealth,
  listenHealthEvents,
  runAllHealthChecks,
  runHealthChecks,
} from "./api";

vi.mock("./api", () => ({
  listHealthSummary: vi.fn(),
  listInstallationHealth: vi.fn(),
  runHealthChecks: vi.fn(),
  runAllHealthChecks: vi.fn(),
  cancelHealthRun: vi.fn(),
  listenHealthEvents: vi.fn(),
}));

const emptyWorkspace = {
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
  installations: [],
  revision: "health-r1",
};

describe("useHealth orchestration", () => {
  beforeEach(() => {
    vi.mocked(listHealthSummary).mockReset();
    vi.mocked(listInstallationHealth).mockReset();
    vi.mocked(runHealthChecks).mockReset();
    vi.mocked(runAllHealthChecks).mockReset();
    vi.mocked(cancelHealthRun).mockReset();
    vi.mocked(listenHealthEvents).mockReset();

    vi.mocked(listHealthSummary).mockResolvedValue(emptyWorkspace as never);
    vi.mocked(listenHealthEvents).mockResolvedValue(vi.fn());

    vi.mocked(runHealthChecks).mockResolvedValue({
      runId: "run-1",
      scope: "installation",
      state: "running",
      revision: "run-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    } as never);
  });

  afterEach(cleanup);

  it("health_frontend_strict_mode_remount_still_allows_single_run", async () => {
    const { result } = renderHook(() => useHealth(), {
      reactStrictMode: true,
    });

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(runHealthChecks).toHaveBeenCalledTimes(1);
    expect(runHealthChecks).toHaveBeenCalledWith("inst-1", "");
  });
  it("health_frontend_listener_is_ready_before_single_run_invoke", async () => {
    const order: string[] = [];

    vi.mocked(listenHealthEvents).mockImplementation(async () => {
      order.push("listener");
      return (() => {}) as () => void;
    });

    vi.mocked(runHealthChecks).mockImplementation(async () => {
      order.push("run");
      return {
        runId: "run-1",
        scope: "installation",
        state: "running",
        revision: "run-r1",
        startedAt: "2026-01-01T00:00:00Z",
        requestedInstallationCount: 1,
        scheduledInstallationCount: 1,
        skippedActiveInstallationCount: 0,
      } as never;
    });

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(order).toEqual(["listener", "run"]);
  });

  it("health_frontend_listener_is_ready_before_check_all_invoke", async () => {
  const order: string[] = [];

  vi.mocked(listenHealthEvents).mockImplementation(async () => {
    order.push("listener");
    return (() => {}) as () => void;
  });

  vi.mocked(runAllHealthChecks).mockImplementation(async () => {
    order.push("run-all");
    return {
      runId: "run-all-1",
      scope: "all",
      state: "running",
      revision: "run-all-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    } as never;
  });

  const { result } = renderHook(() => useHealth());

  await act(async () => {
    await result.current.runAll();
  });

  expect(order).toEqual(["listener", "run-all"]);
});

  it("health_frontend_buffers_event_before_handle_registration", async () => {
    let listener: ((event: unknown) => void) | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(runHealthChecks).mockImplementation(async () => {
      listener?.({
        kind: "completed",
        runId: "run-early",
        completedInstallationCount: 1,
      });

      return {
        runId: "run-early",
        scope: "installation",
        state: "running",
        revision: "run-r1",
        startedAt: "2026-01-01T00:00:00Z",
        requestedInstallationCount: 1,
        scheduledInstallationCount: 1,
        skippedActiveInstallationCount: 0,
      } as never;
    });

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(result.current.activeRuns["run-early"]).toBeUndefined();
    expect(listHealthSummary).toHaveBeenCalled();
  });
  it("health_frontend_installation_updated_refreshes_workspace_without_ending_active_run", async () => {
    let listener: ((event: unknown) => void) | undefined;

    const refreshedWorkspace = {
      ...emptyWorkspace,
      summary: {
        ...emptyWorkspace.summary,
        greenCount: 1,
        greyCount: 0,
      },
      revision: "health-r2",
    };

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(runAllHealthChecks).mockResolvedValue({
      runId: "run-all-1",
      scope: "all",
      state: "running",
      revision: "run-all-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    } as never);

    vi.mocked(listHealthSummary).mockResolvedValue(refreshedWorkspace as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runAll();
    });

    expect(result.current.activeRuns["run-all-1"]).toBeDefined();

    act(() => {
      listener?.({
        kind: "installation_updated",
        runId: "run-all-1",
        installationId: "inst-1",
        aggregate: "green",
      });
    });

    await waitFor(() => {
      expect(listHealthSummary).toHaveBeenCalledTimes(1);
      expect(result.current.workspace?.revision).toBe("health-r2");
      expect(result.current.workspace?.summary.greenCount).toBe(1);
      expect(result.current.activeRuns["run-all-1"]).toBeDefined();
    });
  });
  it("health_frontend_terminal_completed_refreshes_backend_authority", async () => {
    let listener: ((event: unknown) => void) | undefined;

    const refreshedWorkspace = {
      ...emptyWorkspace,
      summary: {
        ...emptyWorkspace.summary,
        greenCount: 1,
        greyCount: 0,
      },
      revision: "health-r2",
    };

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(listHealthSummary).mockResolvedValue(refreshedWorkspace as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(result.current.activeRuns["run-1"]).toBeDefined();

    act(() => {
      listener?.({
        kind: "completed",
        runId: "run-1",
        completedInstallationCount: 1,
      });
    });

    await waitFor(() => {
      expect(result.current.activeRuns["run-1"]).toBeUndefined();
      expect(result.current.workspace?.revision).toBe("health-r2");
      expect(result.current.workspace?.summary.greenCount).toBe(1);
    });
  });
  it("health_frontend_selected_installation_refreshes_after_terminal", async () => {
    let listener: ((event: unknown) => void) | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(listInstallationHealth)
      .mockResolvedValueOnce({
        snapshot: {
          installation: { id: "inst-1" },
        },
        history: [],
      } as never)
      .mockResolvedValueOnce({
        snapshot: {
          installation: { id: "inst-1" },
        },
        history: [{ id: "result-after-run" }],
      } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.selectInstallation("inst-1");
    });

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    act(() => {
      listener?.({
        kind: "completed",
        runId: "run-1",
        completedInstallationCount: 1,
      });
    });

    await waitFor(() => {
      expect(listInstallationHealth).toHaveBeenCalledTimes(2);
      expect(result.current.selected?.history).toHaveLength(1);
    });
  });
  it("health_frontend_cancel_uses_current_run_revision", async () => {
    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    vi.mocked(cancelHealthRun).mockResolvedValue({
      runId: "run-1",
      state: "cancelling",
      revision: "run-r2",
    } as never);

    await act(async () => {
      await result.current.cancelRun("run-1");
    });

    expect(cancelHealthRun).toHaveBeenCalledWith("run-1", "run-r1");
  });
  it("health_frontend_cancel_response_updates_run_revision", async () => {
    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    vi.mocked(cancelHealthRun).mockResolvedValue({
      runId: "run-1",
      state: "cancelling",
      revision: "run-r2",
    } as never);

    await act(async () => {
      await result.current.cancelRun("run-1");
    });

    expect(result.current.activeRuns["run-1"]?.revision).toBe("run-r2");
    expect(result.current.activeRuns["run-1"]?.state).toBe("cancelling");
  });
  it("health_frontend_terminal_cancelled_refreshes_backend_authority", async () => {
    let listener: ((event: unknown) => void) | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(listHealthSummary)
      .mockResolvedValueOnce({
        ...emptyWorkspace,
        revision: "health-r2",
        summary: {
          ...emptyWorkspace.summary,
          greyCount: 0,
          yellowCount: 1,
        },
      } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    act(() => {
      listener?.({
        kind: "cancelled",
        runId: "run-1",
        completedInstallationCount: 1,
      });
    });

    await waitFor(() => {
      expect(result.current.activeRuns["run-1"]).toBeUndefined();
      expect(result.current.workspace?.revision).toBe("health-r2");
      expect(result.current.workspace?.summary.yellowCount).toBe(1);
    });
  });
  it("health_frontend_completed_results_survive_cancel", async () => {
    let listener: ((event: unknown) => void) | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(listInstallationHealth)
      .mockResolvedValueOnce({
        snapshot: {
          installation: { id: "inst-1" },
        },
        history: [],
      } as never)
      .mockResolvedValueOnce({
        snapshot: {
          installation: { id: "inst-1" },
        },
        history: [
          {
            id: "result-1",
            runId: "run-1",
            installationId: "inst-1",
            state: "green",
          },
        ],
      } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.selectInstallation("inst-1");
      await result.current.runInstallation("inst-1");
    });

    act(() => {
      listener?.({
        kind: "check_completed",
        runId: "run-1",
        result: {
          id: "result-1",
          runId: "run-1",
          installationId: "inst-1",
          state: "green",
        },
      });

      listener?.({
        kind: "cancelled",
        runId: "run-1",
        completedInstallationCount: 1,
      });
    });

    await waitFor(() => {
      expect(result.current.selected?.history).toHaveLength(1);
      expect(result.current.selected?.history[0]?.id).toBe("result-1");
    });
  });
  it("health_frontend_run_conflict_refreshes_without_auto_retry", async () => {
    vi.mocked(runHealthChecks).mockRejectedValueOnce({
      code: "conflict",
      message: "Health revision changed",
    });

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      ...emptyWorkspace,
      revision: "health-r2",
    } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(runHealthChecks).toHaveBeenCalledTimes(1);
    expect(listHealthSummary).toHaveBeenCalledTimes(1);
    expect(result.current.workspace?.revision).toBe("health-r2");
    expect(result.current.error).toBeTruthy();
  });

  it("health_frontend_run_all_conflict_refreshes_without_auto_retry", async () => {
    vi.mocked(runAllHealthChecks).mockRejectedValueOnce({
      code: "conflict",
      message: "Health revision changed",
    });

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      ...emptyWorkspace,
      revision: "health-r2",
    } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runAll();
    });

    expect(runAllHealthChecks).toHaveBeenCalledTimes(1);
    expect(listHealthSummary).toHaveBeenCalledTimes(1);
    expect(result.current.workspace?.revision).toBe("health-r2");
    expect(result.current.error).toBeTruthy();
  });
  it("health_frontend_cancel_conflict_refreshes_without_auto_retry", async () => {
    vi.mocked(cancelHealthRun).mockRejectedValueOnce({
      code: "conflict",
      message: "Health run revision changed",
    });

    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      ...emptyWorkspace,
      revision: "health-r2",
    } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    await act(async () => {
      await result.current.cancelRun("run-1");
    });

    expect(cancelHealthRun).toHaveBeenCalledTimes(1);
    expect(listHealthSummary).toHaveBeenCalledTimes(1);
    expect(result.current.workspace?.revision).toBe("health-r2");
    expect(result.current.error).toBeTruthy();
  });
  it("health_frontend_active_error_preserves_existing_run", async () => {
    vi.mocked(runHealthChecks)
      .mockResolvedValueOnce({
        runId: "run-1",
        scope: "installation",
        state: "running",
        revision: "run-r1",
        startedAt: "2026-01-01T00:00:00Z",
        requestedInstallationCount: 1,
        scheduledInstallationCount: 1,
        skippedActiveInstallationCount: 0,
      } as never)
      .mockRejectedValueOnce({
        code: "health_run_active",
        message: "Installation already has an active health run",
      });

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(runHealthChecks).toHaveBeenCalledTimes(2);
    expect(Object.keys(result.current.activeRuns)).toEqual(["run-1"]);
    expect(result.current.activeRuns["run-1"]?.revision).toBe("run-r1");
    expect(result.current.error).toBeTruthy();
  });
  it("health_frontend_unmount_during_terminal_refresh_stops_followup_state_work", async () => {
    let listener: ((event: unknown) => void) | undefined;
    let resolveSummary:
      | ((value: typeof emptyWorkspace) => void)
      | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(async (onEvent) => {
      listener = onEvent as (event: unknown) => void;
      return (() => {}) as () => void;
    });

    vi.mocked(listInstallationHealth).mockResolvedValue({
      snapshot: {
        installation: { id: "inst-1" },
      },
      history: [],
    } as never);

    vi.mocked(listHealthSummary).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSummary = resolve as (value: typeof emptyWorkspace) => void;
        }),
    );

    const { result, unmount } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.selectInstallation("inst-1");
      await result.current.runInstallation("inst-1");
    });

    act(() => {
      listener?.({
        kind: "completed",
        runId: "run-1",
        completedInstallationCount: 1,
      });
    });

    unmount();

    await act(async () => {
      resolveSummary?.(emptyWorkspace);
      await Promise.resolve();
    });

    expect(listInstallationHealth).toHaveBeenCalledTimes(1);
  });
  it("health_frontend_unmount_removes_health_listener", async () => {
    const unlisten = vi.fn();
    let resolveListener: ((unlisten: () => void) => void) | undefined;

    vi.mocked(listenHealthEvents).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveListener = resolve;
        }),
    );

    const { result, unmount } = renderHook(() => useHealth());

    const runPromise = result.current.runInstallation("inst-1");

    unmount();

    resolveListener?.(unlisten);
    await runPromise;

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(runHealthChecks).not.toHaveBeenCalled();
  });
  it("health_frontend_refresh_loads_backend_authority", async () => {
    vi.mocked(listHealthSummary).mockResolvedValueOnce({
      ...emptyWorkspace,
      revision: "health-r2",
    } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.refresh();
    });

    expect(listHealthSummary).toHaveBeenCalledTimes(1);
    expect(runHealthChecks).not.toHaveBeenCalled();
    expect(runAllHealthChecks).not.toHaveBeenCalled();
    expect(result.current.workspace?.revision).toBe("health-r2");
  });
  it("health_frontend_refresh_tracks_loading_state", async () => {
    let resolveSummary:
      | ((value: typeof emptyWorkspace) => void)
      | undefined;

    vi.mocked(listHealthSummary).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSummary = resolve as (value: typeof emptyWorkspace) => void;
        }),
    );

    const { result } = renderHook(() => useHealth());

    let refreshPromise: Promise<void>;

    act(() => {
      refreshPromise = result.current.refresh();
    });

    expect(result.current.loading).toBe(true);

    await act(async () => {
      resolveSummary?.(emptyWorkspace);
      await refreshPromise!;
    });

    expect(result.current.loading).toBe(false);
  });
  it("health_frontend_single_run_uses_current_workspace_revision", async () => {
    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.refresh();
    });

    await act(async () => {
      await result.current.runInstallation("inst-1");
    });

    expect(runHealthChecks).toHaveBeenCalledWith("inst-1", "health-r1");
  });
  it("health_frontend_check_all_uses_current_workspace_revision", async () => {
    vi.mocked(runAllHealthChecks).mockResolvedValueOnce({
      runId: "run-all-1",
      scope: "all",
      state: "running",
      revision: "run-all-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    } as never);

    const { result } = renderHook(() => useHealth());

    await act(async () => {
      await result.current.refresh();
    });

    await act(async () => {
      await result.current.runAll();
    });

    expect(runAllHealthChecks).toHaveBeenCalledWith("health-r1");
  });
});
