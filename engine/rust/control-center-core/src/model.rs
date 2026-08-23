use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Mcp,
    Plugin,
    Skill,
    Launcher,
    LocalService,
    WindowsService,
    Cli,
    DesktopApplication,
    DockerContainer,
    Runtime,
    Configuration,
    Adapter,
    Collection,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Pending,
    Imported,
    Ignored,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedState {
    Detected,
    Installed,
    Missing,
    Running,
    Stopped,
    Enabled,
    Disabled,
    Registered,
    NotRegistered,
    Connected,
    Disconnected,
    Healthy,
    Degraded,
    Failed,
    NotApplicable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Green,
    Yellow,
    Red,
    Grey,
    Blue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    PathExists,
    FileExists,
    ConfigPresent,
    ConfigParses,
    ProcessRunning,
    WindowsServiceState,
    LoopbackPortReachable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthExpectedCondition {
    Exists,
    Parses,
    Running,
    Reachable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HealthTarget {
    Path { path: PathBuf },
    Config { path: PathBuf, format: ConfigFormat },
    ProcessExecutable { path: PathBuf },
    WindowsService { service_name: String },
    LoopbackSocket { host: String, port: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckHint {
    pub check_type: HealthCheckType,
    pub required: bool,
    pub expected_condition: HealthExpectedCondition,
    pub timeout_ms: u64,
    pub stale_after_seconds: Option<u64>,
    pub target: HealthTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Requires,
    DependsOn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyProposal {
    pub kind: DependencyKind,
    pub target_stable_identity: String,
    pub evidence_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalCapability {
    pub id: Uuid,
    pub canonical_name: String,
    pub stable_identity: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallationInstance {
    pub id: Uuid,
    pub capability_id: Uuid,
    pub source_discovery_id: Uuid,
    pub kind: ToolKind,
    pub display_name: String,
    pub host: String,
    pub installation_state: ObservedState,
    pub registration_state: ObservedState,
    pub enablement_state: ObservedState,
    pub runtime_state: ObservedState,
    pub connection_state: ObservedState,
    pub authentication_state: ObservedState,
    pub aggregate_health: HealthState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckDefinition {
    pub id: Uuid,
    pub installation_id: Uuid,
    pub check_type: HealthCheckType,
    pub required: bool,
    pub expected_condition: HealthExpectedCondition,
    pub timeout_ms: u64,
    pub stale_after_seconds: Option<u64>,
    #[serde(rename = "target")]
    pub target_metadata: HealthTarget,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthRunScope {
    Installation,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthRunState {
    Running,
    Cancelling,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthRun {
    pub id: Uuid,
    pub scope: HealthRunScope,
    pub state: HealthRunState,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub requested_installation_count: u64,
    pub completed_installation_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    pub id: Uuid,
    pub run_id: Uuid,
    pub installation_id: Uuid,
    pub check_definition_id: Uuid,
    pub check_type: HealthCheckType,
    pub required: bool,
    pub state: HealthState,
    pub result_code: String,
    pub redacted_evidence: String,
    pub duration_ms: u64,
    pub observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckSnapshot {
    pub definition: HealthCheckDefinition,
    pub latest_result: Option<HealthResult>,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationHealthSnapshot {
    pub installation: InstallationInstance,
    pub capability: CanonicalCapability,
    pub checks: Vec<HealthCheckSnapshot>,
    pub effective_health: HealthState,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub warning_count: u64,
    pub failure_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSummary {
    pub capability_count: u64,
    pub installation_count: u64,
    pub pending_discovery_count: u64,
    pub green_count: u64,
    pub yellow_count: u64,
    pub red_count: u64,
    pub grey_count: u64,
    pub blue_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyOwner {
    Scanner,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyRelationship {
    pub id: Uuid,
    pub source_capability_id: Uuid,
    pub target_capability_id: Uuid,
    pub kind: DependencyKind,
    pub owner: DependencyOwner,
    pub provenance: String,
    pub evidence_summary: String,
    pub created_at: DateTime<Utc>,
    pub revision: String,
}

pub const DEFAULT_HEALTH_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Evidence {
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Discovery {
    pub id: Uuid,
    pub fingerprint: String,
    pub suggested_name: String,
    pub suggested_type: ToolKind,
    pub source_scanner: String,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub observed_at: DateTime<Utc>,
    pub review_state: ReviewState,
    pub installation_state: ObservedState,
    pub registration_state: ObservedState,
    pub enablement_state: ObservedState,
    pub runtime_state: ObservedState,
    pub connection_state: ObservedState,
    pub authentication_state: ObservedState,
    pub health_state: ObservedState,
    #[serde(default)]
    pub canonical_identity: Option<String>,
    #[serde(default)]
    pub health_hints: Vec<HealthCheckHint>,
    #[serde(default)]
    pub dependency_proposals: Vec<DependencyProposal>,
}

impl Discovery {
    pub fn unknown(
        name: impl Into<String>,
        scanner: impl Into<String>,
        fingerprint: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            fingerprint,
            suggested_name: name.into(),
            suggested_type: ToolKind::Unknown,
            source_scanner: scanner.into(),
            confidence: Confidence::Low,
            evidence: Vec::new(),
            observed_at: Utc::now(),
            review_state: ReviewState::Pending,
            installation_state: ObservedState::Detected,
            registration_state: ObservedState::Unknown,
            enablement_state: ObservedState::Unknown,
            runtime_state: ObservedState::Unknown,
            connection_state: ObservedState::Unknown,
            authentication_state: ObservedState::Unknown,
            health_state: ObservedState::Unknown,
            canonical_identity: None,
            health_hints: Vec::new(),
            dependency_proposals: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_discovery_payload_defaults_m7_metadata() {
        let payload = r#"{
            "id":"00000000-0000-0000-0000-000000000001",
            "fingerprint":"legacy",
            "suggested_name":"Legacy Tool",
            "suggested_type":"cli",
            "source_scanner":"windows.path",
            "confidence":"medium",
            "evidence":[],
            "observed_at":"2026-08-22T00:00:00Z",
            "review_state":"pending",
            "installation_state":"detected",
            "registration_state":"unknown",
            "enablement_state":"unknown",
            "runtime_state":"unknown",
            "connection_state":"unknown",
            "authentication_state":"unknown",
            "health_state":"unknown"
        }"#;

        let discovery: Discovery = serde_json::from_str(payload).unwrap();

        assert_eq!(discovery.canonical_identity, None);
        assert!(discovery.health_hints.is_empty());
        assert!(discovery.dependency_proposals.is_empty());
    }

    #[test]
    fn m7_wire_values_are_stable_snake_case() {
        assert_eq!(
            serde_json::to_string(&HealthState::Grey).unwrap(),
            r#""grey""#
        );
        assert_eq!(
            serde_json::to_string(&HealthCheckType::LoopbackPortReachable).unwrap(),
            r#""loopback_port_reachable""#
        );
        assert_eq!(
            serde_json::to_string(&DependencyKind::DependsOn).unwrap(),
            r#""depends_on""#
        );
        assert_eq!(
            serde_json::to_string(&DependencyOwner::Scanner).unwrap(),
            r#""scanner""#
        );
    }
}
