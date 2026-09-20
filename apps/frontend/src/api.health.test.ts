import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { cancelHealthRun, listHealthSummary, listInstallationHealth, listenHealthEvents, reviewDiscovery, runAllHealthChecks, runHealthChecks } from "./api";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

describe("health API adapter", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockReset();

    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  it("health_api_desktop_lists_health_summary", async () => {
    const snapshot = {
      summary: {
        capabilityCount: 0,
        installationCount: 0,
        pendingDiscoveryCount: 0,
        greenCount: 0,
        yellowCount: 0,
        redCount: 0,
        greyCount: 0,
        blueCount: 0,
      },
      installations: [],
      revision: "health-r1",
    };

    vi.mocked(invoke).mockResolvedValueOnce(snapshot);

    await expect(listHealthSummary()).resolves.toEqual(snapshot);
    expect(invoke).toHaveBeenCalledWith("list_health_summary");
  });
  it("health_api_desktop_lists_installation_health", async () => {
    const detail = {
      snapshot: {
        installation: {
          id: "inst-1",
          capabilityId: "cap-1",
          sourceDiscoveryId: "disc-1",
          kind: "runtime",
          displayName: "Example runtime",
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
          id: "cap-1",
          canonicalName: "Example runtime",
          stableIdentity: "demo:1",
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
    };

    vi.mocked(invoke).mockResolvedValueOnce(detail);

    await expect(listInstallationHealth("inst-1")).resolves.toEqual(detail);
    expect(invoke).toHaveBeenCalledWith("list_installation_health", {
      installationId: "inst-1",
    });
  });
  it("health_api_desktop_runs_installation_checks", async () => {
    const handle = {
      runId: "run-1",
      scope: "installation",
      state: "running",
      revision: "run-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 1,
      scheduledInstallationCount: 1,
      skippedActiveInstallationCount: 0,
    };

    vi.mocked(invoke).mockResolvedValueOnce(handle);

    await expect(runHealthChecks("inst-1", "health-r1")).resolves.toEqual(handle);
    expect(invoke).toHaveBeenCalledWith("run_health_checks", {
      request: {
        installationId: "inst-1",
        revision: "health-r1",
      },
    });
  });
  it("health_api_desktop_runs_all_checks", async () => {
    const handle = {
      runId: "run-all-1",
      scope: "all",
      state: "running",
      revision: "run-all-r1",
      startedAt: "2026-01-01T00:00:00Z",
      requestedInstallationCount: 2,
      scheduledInstallationCount: 2,
      skippedActiveInstallationCount: 0,
    };

    vi.mocked(invoke).mockResolvedValueOnce(handle);

    await expect(runAllHealthChecks("health-r1")).resolves.toEqual(handle);
    expect(invoke).toHaveBeenCalledWith("run_all_health_checks", {
      request: {
        revision: "health-r1",
      },
    });
  });
  it("health_api_desktop_cancels_run", async () => {
    const response = {
      runId: "run-1",
      state: "cancelling",
      revision: "run-r2",
    };

    vi.mocked(invoke).mockResolvedValueOnce(response);

    await expect(cancelHealthRun("run-1", "run-r1")).resolves.toEqual(response);
    expect(invoke).toHaveBeenCalledWith("cancel_health_run", {
      request: {
        runId: "run-1",
        revision: "run-r1",
      },
    });
  });
  it("health_api_desktop_listens_to_health_events", async () => {
    const unlisten = vi.fn();
    const listener = vi.fn();

    vi.mocked(listen).mockImplementationOnce(async (eventName, handler) => {
      expect(eventName).toBe("health:event");

      handler({
        event: "health:event",
        id: 1,
        payload: {
          kind: "started",
          runId: "run-1",
          scope: "installation",
          requestedInstallationCount: 1,
        },
      } as never);

      return unlisten;
    });

    await expect(listenHealthEvents(listener)).resolves.toBe(unlisten);

    expect(listener).toHaveBeenCalledWith({
      kind: "started",
      runId: "run-1",
      scope: "installation",
      requestedInstallationCount: 1,
    });
  });
  it("health_frontend_browser_demo_requires_explicit_run", async () => {
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useFakeTimers();

    try {
      await reviewDiscovery("demo-1", "import");

      await vi.advanceTimersByTimeAsync(5_000);

      const detail = await listInstallationHealth("inst-demo-1");

      expect(detail.snapshot.effectiveHealth).toBe("grey");
      expect(detail.snapshot.lastCheckedAt).toBeNull();
      expect(detail.history).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });
  it("health_frontend_browser_demo_check_all_runs_reviewed_installations", async () => {
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useFakeTimers();

    const events: Array<{ kind: string; runId: string }> = [];

    try {
      await reviewDiscovery("demo-1", "import");
      await reviewDiscovery("demo-2", "import");

      const unlisten = await listenHealthEvents((event) => {
        events.push(event);
      });

      const handle = await runAllHealthChecks("demo-health-revision");

      expect(handle.scope).toBe("all");
      expect(handle.requestedInstallationCount).toBe(2);
      expect(handle.scheduledInstallationCount).toBe(2);
      expect(events[0]).toMatchObject({
        kind: "started",
        runId: handle.runId,
      });

      await vi.advanceTimersByTimeAsync(5_000);

      expect(events.some((event) => event.kind === "check_completed")).toBe(true);
      expect(events.some((event) => event.kind === "installation_updated")).toBe(true);
      expect(events.some((event) => event.kind === "completed")).toBe(true);

      const first = await listInstallationHealth("inst-demo-1");
      const second = await listInstallationHealth("inst-demo-2");

      expect(first.snapshot.effectiveHealth).toBe("green");
      expect(second.snapshot.effectiveHealth).toBe("green");
      expect(first.history.length).toBeGreaterThan(0);
      expect(second.history.length).toBeGreaterThan(0);

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });
  it("health_frontend_browser_demo_summary_reflects_explicit_results", async () => {
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useFakeTimers();

    try {
      await reviewDiscovery("demo-1", "import");
      await reviewDiscovery("demo-2", "import");

      const handle = await runAllHealthChecks("demo-health-revision");
      await vi.advanceTimersByTimeAsync(5_000);

      const summary = await listHealthSummary();

      expect(summary.installations).toHaveLength(handle.scheduledInstallationCount);
      expect(summary.summary.installationCount).toBe(handle.scheduledInstallationCount);
      expect(summary.summary.greenCount).toBe(handle.scheduledInstallationCount);
      expect(summary.summary.greyCount).toBe(0);
      expect(summary.installations.every((item) => item.effectiveHealth === "green")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
  it("health_frontend_browser_demo_unlisten_clears_pending_timers", async () => {
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useFakeTimers();

    try {
      await reviewDiscovery("demo-1", "import");

      const before = (await listInstallationHealth("inst-demo-1")).history.length;
      const unlisten = await listenHealthEvents(() => {});

      await runHealthChecks("inst-demo-1", "demo-health-revision");
      unlisten();

      await vi.advanceTimersByTimeAsync(5_000);

      const after = (await listInstallationHealth("inst-demo-1")).history.length;
      expect(after).toBe(before);
    } finally {
      vi.useRealTimers();
    }
  });
  it("health_frontend_browser_demo_cancel_stops_future_results", async () => {
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useFakeTimers();

    const events: Array<{ kind: string; runId: string }> = [];

    try {
      await reviewDiscovery("demo-2", "import");

      const unlisten = await listenHealthEvents((event) => {
        events.push(event);
      });

      const historyBeforeRun = (await listInstallationHealth("inst-demo-2")).history.length;
      const handle = await runHealthChecks("inst-demo-2", "demo-health-revision");

      await vi.advanceTimersByTimeAsync(60);

      expect(events.filter((event) => event.kind === "check_completed")).toHaveLength(1);

      const cancelled = await cancelHealthRun(handle.runId, handle.revision);
      expect(cancelled.state).toBe("cancelling");

      const completedBeforeAdvance = events.filter(
        (event) => event.kind === "check_completed",
      ).length;

      await vi.advanceTimersByTimeAsync(5_000);

      expect(
        events.filter((event) => event.kind === "check_completed"),
      ).toHaveLength(completedBeforeAdvance);
      expect(events.some((event) => event.kind === "cancelled")).toBe(true);

      const detail = await listInstallationHealth("inst-demo-2");
      expect(detail.history).toHaveLength(historyBeforeRun + 1);

      unlisten();
    } finally {
      vi.useRealTimers();
    }
  });
});
