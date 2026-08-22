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

export type BootstrapState = {
  mode: "desktop" | "demo";
  pending: Discovery[];
  inventory: Discovery[];
  scanRevision: string;
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
