export type PageName = "Overview" | "Inventory" | "Review Queue" | "Health" | "Dependencies" | "Activity" | "Adapter Packs" | "Backups" | "Settings";

export type Discovery = {
  id: string;
  suggested_name: string;
  suggested_type: string;
  source_scanner: string;
  confidence: "low" | "medium" | "high";
  evidence: { kind: string; summary: string }[];
  observed_at: string;
  health_state: string;
};

export type HealthState = "green" | "yellow" | "red" | "grey" | "blue";
export type HealthCheckType =
  | "path_exists"
  | "file_exists"
  | "config_present"
  | "config_parses"
  | "process_running"
  | "windows_service_state"
  | "loopback_port_reachable";

export type ConfigFormat = "json";

export type HealthExpectedCondition = "exists" | "parses" | "running" | "reachable";

export type HealthTarget =
  | { kind: "path"; path: string }
  | { kind: "config"; path: string; format: ConfigFormat }
  | { kind: "process_executable"; path: string }
  | { kind: "windows_service"; serviceName: string }
  | { kind: "loopback_socket"; host: string; port: number };

export interface HealthCheckDefinition {
  id: string;
  installationId: string;
  checkType: HealthCheckType;
  required: boolean;
  expectedCondition: HealthExpectedCondition;
  timeoutMs: number;
  staleAfterSeconds: number | null;
  target: HealthTarget;
  createdAt: string;
  updatedAt: string;
}

export type HealthRunScope = "installation" | "all";

export type HealthRunState = "running" | "cancelling" | "cancelled" | "completed";

export interface HealthResult {
  id: string;
  runId: string;
  installationId: string;
  checkDefinitionId: string;
  checkType: HealthCheckType;
  required: boolean;
  state: HealthState;
  resultCode: string;
  redactedEvidence: string;
  durationMs: number;
  observedAt: string;
}

export interface HealthCheckSnapshot {
  definition: HealthCheckDefinition;
  latestResult: HealthResult | null;
  stale: boolean;
}

export interface InstallationHealthSnapshot {
  installation: InstallationInstance;
  capability: CanonicalCapability;
  checks: HealthCheckSnapshot[];
  effectiveHealth: HealthState;
  lastCheckedAt: string | null;
  warningCount: number;
  failureCount: number;
}

export interface HealthSummary {
  capabilityCount: number;
  installationCount: number;
  pendingDiscoveryCount: number;
  greenCount: number;
  yellowCount: number;
  redCount: number;
  greyCount: number;
  blueCount: number;
}

export interface HealthWorkspaceSnapshot {
  summary: HealthSummary;
  installations: InstallationHealthSnapshot[];
  revision: string;
}

export interface InstallationHealthDetail {
  snapshot: InstallationHealthSnapshot;
  history: HealthResult[];
}

export interface HealthRunStateResponse {
  runId: string;
  state: HealthRunState;
  revision: string;
}

export interface HealthRunHandle {
  runId: string;
  scope: HealthRunScope;
  state: HealthRunState;
  revision: string;
  startedAt: string;
  requestedInstallationCount: number;
  scheduledInstallationCount: number;
  skippedActiveInstallationCount: number;
}

export type HealthEvent =
  | {
      kind: "started";
      runId: string;
      scope: HealthRunScope;
      requestedInstallationCount: number;
    }
  | {
      kind: "check_completed";
      runId: string;
      result: HealthResult;
    }
  | {
      kind: "installation_updated";
      runId: string;
      installationId: string;
      aggregate: HealthState;
    }
  | {
      kind: "cancelled";
      runId: string;
      completedInstallationCount: number;
    }
  | {
      kind: "completed";
      runId: string;
      completedInstallationCount: number;
    };

export type DependencyKind = "requires" | "depends_on";
export type DependencyOwner = "scanner" | "user";

export interface DependencyRelationship {
  id: string;
  sourceCapabilityId: string;
  targetCapabilityId: string;
  kind: DependencyKind;
  owner: DependencyOwner;
  provenance: string;
  evidenceSummary: string;
  createdAt: string;
  revision: string;
}

export interface DependencyWorkspaceSnapshot {
  relationships: DependencyRelationship[];
  revision: string;
}

export interface DependencyMutationResponse {
  relationships: DependencyRelationship[];
  revision: string;
}
export interface CanonicalCapability {
  id: string;
  canonicalName: string;
  stableIdentity: string | null;
  createdAt: string;
  updatedAt: string;
  revision: string;
}

export interface InstallationInstance {
  id: string;
  capabilityId: string;
  sourceDiscoveryId: string;
  kind: string;
  displayName: string;
  host: string;
  installationState: string;
  registrationState: string;
  enablementState: string;
  runtimeState: string;
  connectionState: string;
  authenticationState: string;
  aggregateHealth: HealthState;
  createdAt: string;
  updatedAt: string;
}

export type BootstrapState = {
  mode: "desktop" | "demo";
  pending: Discovery[];
  capabilities: CanonicalCapability[];
  installations: InstallationInstance[];
  scanRevision: string;
  healthRevision: string;
  dependencyRevision: string;
};

export type ScanMode = "quick" | "deep";

export type ScanLifecycleState = "running" | "paused" | "cancelled" | "completed" | "failed";

export interface ScanRequest {
  mode: ScanMode;
  roots: string[];
  followReparsePoints: boolean;
  networkConsent: boolean;
  revision: string;
}

export interface ScanMutationRequest {
  scanId: string;
  revision: string;
}

export interface ScanHandle {
  scanId: string;
  scope: ScanMode;
  state: ScanLifecycleState;
  revision: string;
  startedAt: string;
}

export interface ScanState {
  scanId: string;
  state: ScanLifecycleState;
  revision: string;
}

// `ScanEvent` mirrors the Rust `ScanEvent` enum wire shape exactly. Only the
// `kind` discriminant tag is renamed to snake_case; the other fields keep
// their Rust (snake_case) names as serialized, since `ScanEvent` derives
// plain `Serialize` with `#[serde(tag = "kind", rename_all = "snake_case")]`
// and does not apply `rename_all = "camelCase"` to its field names.
export type ScanEvent =
  | { kind: "started"; scope: ScanMode; scanner_count: number }
  | {
      kind: "progress";
      scanner_id: string;
      completed_units: number;
      total_units: number | null | undefined;
      current_location: string | null | undefined;
    }
  | { kind: "discovery"; discovery: Discovery }
  | { kind: "scanner_failed"; scanner_id: string; code: string; message: string }
  | { kind: "paused" }
  | { kind: "resumed" }
  | { kind: "cancelled"; visited: number; discovered: number; failure_count: number; duration_ms: number }
  | { kind: "completed"; visited: number; discovered: number; failure_count: number; duration_ms: number }
  | { kind: "failed"; code: string; message: string; failure_count: number; duration_ms: number };
