use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
        }
    }
}
