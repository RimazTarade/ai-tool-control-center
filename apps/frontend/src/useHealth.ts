import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelHealthRun,
  listHealthSummary,
  listInstallationHealth,
  listenHealthEvents,
  runAllHealthChecks,
  runHealthChecks,
} from "./api";

import type {
  HealthEvent,
  HealthRunHandle,
  HealthWorkspaceSnapshot,
  InstallationHealthDetail,
} from "./model";

export interface UseHealthValue {
  workspace: HealthWorkspaceSnapshot | null;
  selected: InstallationHealthDetail | null;
  activeRuns: Record<string, HealthRunHandle>;
  activeInstallationRuns: Record<string, string>;
  lastTerminalRunState: "completed" | "cancelled" | null;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
  selectInstallation(installationId: string): Promise<void>;
  runInstallation(installationId: string): Promise<void>;
  runAll(): Promise<void>;
  cancelRun(runId: string): Promise<void>;
}

export function useHealth(): UseHealthValue {
  const [workspace, setWorkspace] = useState<HealthWorkspaceSnapshot | null>(null);
  const [selected, setSelected] = useState<InstallationHealthDetail | null>(null);
  const [activeRuns, setActiveRuns] = useState<Record<string, HealthRunHandle>>({});
  const [activeInstallationRuns, setActiveInstallationRuns] = useState<Record<string, string>>({});
  const [lastTerminalRunState, setLastTerminalRunState] = useState<
    "completed" | "cancelled" | null
  >(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const unlistenRef = useRef<(() => void) | null>(null);
  const listenerPromiseRef = useRef<Promise<void> | null>(null);
  const unmountedRef = useRef(false);
  const activeRunIdsRef = useRef(new Set<string>());
  const activeRunInstallationsRef = useRef(new Map<string, string | null>());
  const installationActiveRunRef = useRef(new Map<string, string>());
  const bufferedEventsRef = useRef(new Map<string, HealthEvent[]>());
  const selectedInstallationIdRef = useRef<string | null>(null);

  const handleEvent = useCallback(async (event: HealthEvent) => {
    if (!activeRunIdsRef.current.has(event.runId)) {
      const buffered = bufferedEventsRef.current.get(event.runId) ?? [];
      buffered.push(event);
      bufferedEventsRef.current.set(event.runId, buffered);
      return;
    }

    if (event.kind === "installation_updated") {
      const refreshed = await listHealthSummary();
      if (unmountedRef.current) return;

      setWorkspace(refreshed);

      if (selectedInstallationIdRef.current === event.installationId) {
        const refreshedSelected = await listInstallationHealth(
          event.installationId,
        );
        if (unmountedRef.current) return;
        setSelected(refreshedSelected);
      }

      return;
    }
    if (event.kind === "completed" || event.kind === "cancelled") {
      setLastTerminalRunState(event.kind);

      const runInstallationId = activeRunInstallationsRef.current.get(event.runId) ?? null;

      activeRunIdsRef.current.delete(event.runId);
      activeRunInstallationsRef.current.delete(event.runId);
      if (
        runInstallationId !== null &&
        installationActiveRunRef.current.get(runInstallationId) === event.runId
      ) {
        installationActiveRunRef.current.delete(runInstallationId);
        setActiveInstallationRuns((current) => {
          const next = { ...current };
          delete next[runInstallationId];
          return next;
        });
      }

      setActiveRuns((current) => {
        const next = { ...current };
        delete next[event.runId];
        return next;
      });

      const refreshed = await listHealthSummary();
      if (unmountedRef.current) return;

      setWorkspace(refreshed);

      const selectedInstallationId = selectedInstallationIdRef.current;
      const selectedWasAffected =
        selectedInstallationId !== null &&
        (runInstallationId === null || runInstallationId === selectedInstallationId);

      if (selectedWasAffected) {
        const refreshedSelected = await listInstallationHealth(selectedInstallationId);
        setSelected(refreshedSelected);
      }
    }
  }, []);

  const ensureListener = useCallback(async () => {
    if (unlistenRef.current) return;

    if (!listenerPromiseRef.current) {
      listenerPromiseRef.current = listenHealthEvents((event) => {
        void handleEvent(event);
      }).then((unlisten) => {
        if (unmountedRef.current) {
          unlisten();
          return;
        }

        unlistenRef.current = unlisten;
      });
    }

    await listenerPromiseRef.current;
  }, [handleEvent]);

  const registerRun = useCallback(
    async (handle: HealthRunHandle, installationId: string | null) => {
      setLastTerminalRunState(null);
      activeRunIdsRef.current.add(handle.runId);
      activeRunInstallationsRef.current.set(handle.runId, installationId);
      if (installationId !== null) {
        installationActiveRunRef.current.set(installationId, handle.runId);
        setActiveInstallationRuns((current) => ({
          ...current,
          [installationId]: handle.runId,
        }));
      }

      setActiveRuns((current) => ({
        ...current,
        [handle.runId]: handle,
      }));

      const buffered = bufferedEventsRef.current.get(handle.runId) ?? [];
      bufferedEventsRef.current.delete(handle.runId);

      for (const event of buffered) {
        await handleEvent(event);
      }
    },
    [handleEvent],
  );

  const refresh = useCallback(async () => {
    setLoading(true);

    try {
      const refreshed = await listHealthSummary();
      setWorkspace(refreshed);
    } finally {
      setLoading(false);
    }
  }, []);

  const selectInstallation = useCallback(async (installationId: string) => {
    selectedInstallationIdRef.current = installationId;
    const detail = await listInstallationHealth(installationId);
    setSelected(detail);
  }, []);

  const runInstallation = useCallback(
    async (installationId: string) => {
      await ensureListener();
      if (unmountedRef.current) return;

      try {
        const handle = await runHealthChecks(installationId, workspace?.revision ?? "");
        await registerRun(handle, installationId);
      } catch (caught) {
        if (
          typeof caught === "object" &&
          caught !== null &&
          "code" in caught &&
          caught.code === "conflict"
        ) {
          const refreshed = await listHealthSummary();
          setWorkspace(refreshed);
          setError(
            "message" in caught && typeof caught.message === "string"
              ? caught.message
              : "Health state changed. Refresh and try again.",
          );
          return;
        }

        if (
          typeof caught === "object" &&
          caught !== null &&
          "code" in caught &&
          caught.code === "health_run_active"
        ) {
          setError(
            "message" in caught && typeof caught.message === "string"
              ? caught.message
              : "This installation already has an active health run.",
          );
          return;
        }

        throw caught;
      }
    },
    [ensureListener, registerRun, workspace],
  );

  const runAll = useCallback(async () => {
    await ensureListener();
    if (unmountedRef.current) return;

    try {
      const handle = await runAllHealthChecks(workspace?.revision ?? "");
      await registerRun(handle, null);
    } catch (caught) {
      if (
        typeof caught === "object" &&
        caught !== null &&
        "code" in caught &&
        caught.code === "conflict"
      ) {
        const refreshed = await listHealthSummary();
        setWorkspace(refreshed);
        setError(
          "message" in caught && typeof caught.message === "string"
            ? caught.message
            : "Health state changed. Refresh and try again.",
        );
        return;
      }

      throw caught;
    }
  }, [ensureListener, registerRun, workspace]);

  const cancelRun = useCallback(async (runId: string) => {
    const run = activeRuns[runId];
    if (!run) return;

    try {
      const response = await cancelHealthRun(runId, run.revision);

      setActiveRuns((current) => {
        const existing = current[runId];
        if (!existing) return current;

        return {
          ...current,
          [runId]: {
            ...existing,
            state: response.state,
            revision: response.revision,
          },
        };
      });
    } catch (caught) {
      if (
        typeof caught === "object" &&
        caught !== null &&
        "code" in caught &&
        caught.code === "conflict"
      ) {
        const refreshed = await listHealthSummary();
        setWorkspace(refreshed);
        setError(
          "message" in caught && typeof caught.message === "string"
            ? caught.message
            : "Health run changed. Refresh and try again.",
        );
        return;
      }

      throw caught;
    }
  }, [activeRuns]);

  useEffect(() => {
    // React StrictMode replays mount effects in development. Reset the flag on
    // every setup so the replay cleanup is not mistaken for a real unmount.
    unmountedRef.current = false;

    return () => {
      unmountedRef.current = true;
      unlistenRef.current?.();
    };
  }, []);

  return {
    workspace,
    selected,
    activeRuns,
    activeInstallationRuns,
    lastTerminalRunState,
    loading,
    error,
    refresh,
    selectInstallation,
    runInstallation,
    runAll,
    cancelRun,
  };
}