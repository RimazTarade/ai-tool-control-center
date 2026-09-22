import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootstrapState, CanonicalCapability, DependencyKind, DependencyMutationResponse, DependencyRelationship, DependencyWorkspaceSnapshot, Discovery, HealthEvent, HealthResult, HealthRunHandle, HealthRunStateResponse, HealthWorkspaceSnapshot, InstallationHealthDetail, InstallationInstance, ScanEvent, ScanHandle, ScanMutationRequest, ScanRequest, ScanState } from "./model";

const synthetic: Discovery[] = [
  {
    id: "demo-1",
    suggested_name: "Example local runtime",
    suggested_type: "runtime",
    source_scanner: "demo.fixture",
    confidence: "high",
    evidence: [{ kind: "demo", summary: "Synthetic sample — not observed on this computer" }],
    observed_at: "2026-01-01T00:00:00Z",
    health_state: "unknown",
  },
  {
    id: "demo-2",
    suggested_name: "Example MCP registration",
    suggested_type: "mcp",
    source_scanner: "demo.fixture",
    confidence: "medium",
    evidence: [{ kind: "demo", summary: "Synthetic configuration evidence" }],
    observed_at: "2026-01-01T00:00:00Z",
    health_state: "unknown",
  },
];

let demoPending = [...synthetic];
const demoCapabilities: CanonicalCapability[] = [];
const demoInstallations: InstallationInstance[] = [];

const demoDependencyRelationships: DependencyRelationship[] = [];
let demoDependencyRevisionCounter = 1;
let demoDependencyRevision = "demo-dependency-r1";
let demoDependencyRelationshipCounter = 0;

function rotateDemoDependencyRevision() {
  demoDependencyRevisionCounter += 1;
  demoDependencyRevision = `demo-dependency-r${demoDependencyRevisionCounter}`;
  return demoDependencyRevision;
}

function ensureDemoScannerDependency() {
  const relationshipId = "demo-scanner";
  if (
    demoDependencyRelationships.some(
      (relationship) => relationship.id === relationshipId,
    )
  ) {
    return;
  }

  const sourceExists = demoCapabilities.some(
    (capability) => capability.id === "cap-demo-1",
  );
  const targetExists = demoCapabilities.some(
    (capability) => capability.id === "cap-demo-2",
  );

  if (!sourceExists || !targetExists) return;

  const revision = rotateDemoDependencyRevision();
  demoDependencyRelationships.push({
    id: relationshipId,
    sourceCapabilityId: "cap-demo-1",
    targetCapabilityId: "cap-demo-2",
    kind: "requires",
    owner: "scanner",
    provenance: "demo.fixture",
    evidenceSummary:
      "Explicit synthetic scanner relationship for browser demo behavior.",
    createdAt: "2026-01-01T00:00:01Z",
    revision,
  });
}

function dependencyError(code: string, message: string): never {
  throw { code, message };
}

function normalizeDependencyError(error: unknown) {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string" &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return { code: error.code, message: error.message };
  }

  return {
    code: "dependency_error",
    message: "Unable to update local relationship data.",
  };
}

const demoHealthListeners = new Set<(event: HealthEvent) => void>();
const demoHealthHistory = new Map<string, HealthResult[]>();

type DemoHealthRun = {
  installationIds: string[];
  revision: string;
  timers: ReturnType<typeof setTimeout>[];
  terminal: boolean;
};

const demoHealthRuns = new Map<string, DemoHealthRun>();
let demoHealthRunCounter = 0;

function emitDemoHealth(event: HealthEvent) {
  for (const listener of [...demoHealthListeners]) listener(event);
}

function recordDemoHealthResult(runId: string, installationId: string, sequence: number) {
  const run = demoHealthRuns.get(runId);
  if (!run || run.terminal) return;

  const result: HealthResult = {
    id: `demo-result-${runId}-${installationId}-${sequence}`,
    runId,
    installationId,
    checkDefinitionId: `demo-check-${installationId}-${sequence}`,
    checkType: "path_exists",
    required: true,
    state: "green",
    resultCode: "demo_synthetic_ok",
    redactedEvidence: "Synthetic demo result - not observed on this computer",
    durationMs: 10,
    observedAt: new Date().toISOString(),
  };

  const history = demoHealthHistory.get(installationId) ?? [];
  history.push(result);
  demoHealthHistory.set(installationId, history);

  const installation = demoInstallations.find(
    (candidate) => candidate.id === installationId,
  );
  if (installation) installation.aggregateHealth = "green";

  emitDemoHealth({ kind: "check_completed", runId, result });
  emitDemoHealth({
    kind: "installation_updated",
    runId,
    installationId,
    aggregate: "green",
  });
}

export const isDesktop = () => "__TAURI_INTERNALS__" in window;

export async function bootstrap(): Promise<BootstrapState> {
  if (isDesktop()) return invoke<BootstrapState>("bootstrap_state");
  return {
    mode: "demo",
    pending: [...demoPending],
    capabilities: [...demoCapabilities],
    installations: [...demoInstallations],
    scanRevision: "demo-revision",
    healthRevision: "demo-health-revision",
    dependencyRevision: demoDependencyRevision,
  };
}
export async function listHealthSummary(): Promise<HealthWorkspaceSnapshot> {
  if (isDesktop()) return invoke<HealthWorkspaceSnapshot>("list_health_summary");

  const installations = demoInstallations.flatMap((installation) => {
    const capability = demoCapabilities.find(
      (candidate) => candidate.id === installation.capabilityId,
    );
    if (!capability) return [];

    const history = demoHealthHistory.get(installation.id) ?? [];
    const latestResult = history.length > 0 ? history[history.length - 1] : null;

    return [
      {
        installation,
        capability,
        checks: [],
        effectiveHealth: installation.aggregateHealth,
        lastCheckedAt: latestResult?.observedAt ?? null,
        warningCount: installation.aggregateHealth === "yellow" ? 1 : 0,
        failureCount: installation.aggregateHealth === "red" ? 1 : 0,
      },
    ];
  });

  const count = (state: InstallationInstance["aggregateHealth"]) =>
    installations.filter((item) => item.effectiveHealth === state).length;

  return {
    summary: {
      capabilityCount: demoCapabilities.length,
      installationCount: demoInstallations.length,
      pendingDiscoveryCount: demoPending.length,
      greenCount: count("green"),
      yellowCount: count("yellow"),
      redCount: count("red"),
      greyCount: count("grey"),
      blueCount: count("blue"),
    },
    installations,
    revision: "demo-health-revision",
  };
}
export async function listInstallationHealth(
  installationId: string,
): Promise<InstallationHealthDetail> {
  if (isDesktop()) {
    return invoke<InstallationHealthDetail>("list_installation_health", {
      installationId,
    });
  }

  const installation = demoInstallations.find((candidate) => candidate.id === installationId);
  if (!installation) throw new Error("Demo installation not found");

  const capability = demoCapabilities.find(
    (candidate) => candidate.id === installation.capabilityId,
  );
  if (!capability) throw new Error("Demo capability not found");

  const history = [...(demoHealthHistory.get(installationId) ?? [])];

  return {
    snapshot: {
      installation,
      capability,
      checks: [],
      effectiveHealth: installation.aggregateHealth,
      lastCheckedAt: history.at(-1)?.observedAt ?? null,
      warningCount: 0,
      failureCount: 0,
    },
    history,
  };
}

export async function runHealthChecks(
  installationId: string,
  revision: string,
): Promise<HealthRunHandle> {
  if (isDesktop()) {
    return invoke<HealthRunHandle>("run_health_checks", {
      request: {
        installationId,
        revision,
      },
    });
  }

  const installation = demoInstallations.find(
    (candidate) => candidate.id === installationId,
  );
  if (!installation) throw new Error("Demo installation not found");

  demoHealthRunCounter += 1;
  const runId = `demo-health-run-${demoHealthRunCounter}`;
  const runRevision = `${runId}-r1`;
  const startedAt = new Date().toISOString();

  const run: DemoHealthRun = {
    installationIds: [installationId],
    revision: runRevision,
    timers: [],
    terminal: false,
  };
  demoHealthRuns.set(runId, run);

  emitDemoHealth({
    kind: "started",
    runId,
    scope: "installation",
    requestedInstallationCount: 1,
  });

  run.timers.push(
    setTimeout(() => recordDemoHealthResult(runId, installationId, 1), 50),
    setTimeout(() => recordDemoHealthResult(runId, installationId, 2), 100),
    setTimeout(() => {
      const current = demoHealthRuns.get(runId);
      if (!current || current.terminal) return;

      current.terminal = true;
      emitDemoHealth({
        kind: "completed",
        runId,
        completedInstallationCount: 1,
      });
    }, 150),
  );

  return {
    runId,
    scope: "installation",
    state: "running",
    revision: runRevision,
    startedAt,
    requestedInstallationCount: 1,
    scheduledInstallationCount: 1,
    skippedActiveInstallationCount: 0,
  };
}
export async function runAllHealthChecks(
  revision: string,
): Promise<HealthRunHandle> {
  if (isDesktop()) {
    return invoke<HealthRunHandle>("run_all_health_checks", {
      request: {
        revision,
      },
    });
  }

  demoHealthRunCounter += 1;
  const runId = `demo-health-run-${demoHealthRunCounter}`;
  const runRevision = `${runId}-r1`;
  const startedAt = new Date().toISOString();
  const installationIds = demoInstallations.map((installation) => installation.id);

  const run: DemoHealthRun = {
    installationIds,
    revision: runRevision,
    timers: [],
    terminal: false,
  };
  demoHealthRuns.set(runId, run);

  emitDemoHealth({
    kind: "started",
    runId,
    scope: "all",
    requestedInstallationCount: installationIds.length,
  });

  installationIds.forEach((installationId, index) => {
    run.timers.push(
      setTimeout(
        () => recordDemoHealthResult(runId, installationId, 1),
        50 + index * 50,
      ),
    );
  });

  run.timers.push(
    setTimeout(() => {
      const current = demoHealthRuns.get(runId);
      if (!current || current.terminal) return;

      current.terminal = true;
      emitDemoHealth({
        kind: "completed",
        runId,
        completedInstallationCount: installationIds.length,
      });
    }, 100 + installationIds.length * 50),
  );

  return {
    runId,
    scope: "all",
    state: "running",
    revision: runRevision,
    startedAt,
    requestedInstallationCount: installationIds.length,
    scheduledInstallationCount: installationIds.length,
    skippedActiveInstallationCount: 0,
  };
}
export async function cancelHealthRun(
  runId: string,
  revision: string,
): Promise<HealthRunStateResponse> {
  if (isDesktop()) {
    return invoke<HealthRunStateResponse>("cancel_health_run", {
      request: {
        runId,
        revision,
      },
    });
  }

  const run = demoHealthRuns.get(runId);
  if (!run) throw new Error("Demo health run not found");

  if (run.revision !== revision) {
    throw {
      code: "conflict",
      message: "Health run revision is stale",
      state: null,
    };
  }

  const nextRevision = `${runId}-r2`;
  run.revision = nextRevision;
  run.terminal = true;
  for (const timer of run.timers) clearTimeout(timer);

  emitDemoHealth({
    kind: "cancelled",
    runId,
    completedInstallationCount: 0,
  });

  return {
    runId,
    state: "cancelling",
    revision: nextRevision,
  };
}
export async function listenHealthEvents(
  listener: (event: HealthEvent) => void,
): Promise<UnlistenFn> {
  if (isDesktop()) {
    return listen<HealthEvent>("health:event", ({ payload }) => {
      listener(payload);
    });
  }

  demoHealthListeners.add(listener);
  return () => {
    demoHealthListeners.delete(listener);

    if (demoHealthListeners.size === 0) {
      for (const run of demoHealthRuns.values()) {
        if (run.terminal) continue;

        run.terminal = true;
        for (const timer of run.timers) clearTimeout(timer);
      }
    }
  };
}
export async function listDependencies(): Promise<DependencyWorkspaceSnapshot> {
  if (isDesktop()) {
    try {
      return await invoke<DependencyWorkspaceSnapshot>("list_dependencies");
    } catch (error) {
      throw normalizeDependencyError(error);
    }
  }

  ensureDemoScannerDependency();
  return {
    relationships: [...demoDependencyRelationships],
    revision: demoDependencyRevision,
  };
}

export async function createUserDependency(
  sourceCapabilityId: string,
  kind: DependencyKind,
  targetCapabilityId: string,
  revision: string,
): Promise<DependencyMutationResponse> {
  if (isDesktop()) {
    try {
      return await invoke<DependencyMutationResponse>("create_user_dependency", {
        request: {
          sourceCapabilityId,
          kind,
          targetCapabilityId,
          revision,
        },
      });
    } catch (error) {
      throw normalizeDependencyError(error);
    }
  }

  ensureDemoScannerDependency();

  if (revision !== demoDependencyRevision) {
    dependencyError(
      "conflict",
      "Dependency data changed. Refresh and retry.",
    );
  }

  if (sourceCapabilityId === targetCapabilityId) {
    dependencyError(
      "dependency_self_reference",
      "A capability cannot depend on itself.",
    );
  }

  const sourceExists = demoCapabilities.some(
    (capability) => capability.id === sourceCapabilityId,
  );
  const targetExists = demoCapabilities.some(
    (capability) => capability.id === targetCapabilityId,
  );

  if (!sourceExists || !targetExists) {
    dependencyError(
      "dependency_invalid",
      "Dependency endpoints must be reviewed capabilities.",
    );
  }

  if (
    demoDependencyRelationships.some(
      (relationship) =>
        relationship.sourceCapabilityId === sourceCapabilityId &&
        relationship.kind === kind &&
        relationship.targetCapabilityId === targetCapabilityId,
    )
  ) {
    dependencyError(
      "dependency_duplicate",
      "That relationship already exists.",
    );
  }

  demoDependencyRelationshipCounter += 1;
  const nextRevision = rotateDemoDependencyRevision();

  demoDependencyRelationships.push({
    id: `demo-user-${demoDependencyRelationshipCounter}`,
    sourceCapabilityId,
    targetCapabilityId,
    kind,
    owner: "user",
    provenance: "user",
    evidenceSummary: "User-created synthetic demo relationship.",
    createdAt: new Date().toISOString(),
    revision: nextRevision,
  });

  return {
    relationships: [...demoDependencyRelationships],
    revision: nextRevision,
  };
}

export async function deleteUserDependency(
  relationshipId: string,
  revision: string,
): Promise<DependencyMutationResponse> {
  if (isDesktop()) {
    try {
      return await invoke<DependencyMutationResponse>("delete_user_dependency", {
        request: {
          relationshipId,
          revision,
        },
      });
    } catch (error) {
      throw normalizeDependencyError(error);
    }
  }

  ensureDemoScannerDependency();

  if (revision !== demoDependencyRevision) {
    dependencyError(
      "conflict",
      "Dependency data changed. Refresh and retry.",
    );
  }

  const index = demoDependencyRelationships.findIndex(
    (relationship) => relationship.id === relationshipId,
  );

  if (index < 0) {
    dependencyError("not_found", "Dependency relationship not found.");
  }

  if (demoDependencyRelationships[index].owner !== "user") {
    dependencyError(
      "dependency_not_user_owned",
      "Scanner relationships are read-only.",
    );
  }

  demoDependencyRelationships.splice(index, 1);
  const nextRevision = rotateDemoDependencyRevision();

  return {
    relationships: [...demoDependencyRelationships],
    revision: nextRevision,
  };
}
export async function reviewDiscovery(id: string, decision: "import" | "ignore" | "unknown"): Promise<void> {
  if (isDesktop()) {
    await invoke("review_discovery", { id, decision });
    return;
  }

  const item = demoPending.find((candidate) => candidate.id === id);
  demoPending = demoPending.filter((candidate) => candidate.id !== id);

  if (decision !== "import" || !item) return;

  const capabilityId = `cap-${item.id}`;
  const installationId = `inst-${item.id}`;

  demoCapabilities.push({
    id: capabilityId,
    canonicalName: item.suggested_name,
    stableIdentity: `demo:${item.id}`,
    createdAt: item.observed_at,
    updatedAt: item.observed_at,
    revision: `demo-capability-${item.id}`,
  });

  demoInstallations.push({
    id: installationId,
    capabilityId,
    sourceDiscoveryId: item.id,
    kind: item.suggested_type,
    displayName: item.suggested_name,
    host: "localhost",
    installationState: "installed",
    registrationState: "registered",
    enablementState: "enabled",
    runtimeState: "unknown",
    connectionState: "unknown",
    authenticationState: "unknown",
    aggregateHealth: "grey",
    createdAt: item.observed_at,
    updatedAt: item.observed_at,
  });

  ensureDemoScannerDependency();
}

export const pickScanRoots = () => invoke<string[]>("pick_scan_roots");

/// Attaches the `scan:event` listener before invoking `start_scan` so an
/// immediate `started` event emitted synchronously by the backend cannot be
/// missed by the frontend.
export async function startScan(
  request: ScanRequest,
  onEvent: (event: ScanEvent) => void,
): Promise<{ handle: ScanHandle; unlisten: UnlistenFn }> {
  const unlisten = await listen<ScanEvent>("scan:event", ({ payload }) => {
    onEvent(payload);
  });

  try {
    const handle = await invoke<ScanHandle>("start_scan", { request });
    return { handle, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export const pauseScan = (request: ScanMutationRequest) => invoke<ScanState>("pause_scan", { request });

export const resumeScan = (request: ScanMutationRequest) => invoke<ScanState>("resume_scan", { request });

export const cancelScan = (request: ScanMutationRequest) => invoke<ScanState>("cancel_scan", { request });
