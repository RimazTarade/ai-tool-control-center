use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use control_center_core::{
    HealthEventSink, HealthResult, HealthRun, HealthRunScope, HealthRunState, HealthState,
    HealthSummary, InstallationHealthSnapshot, InstallationHealthWork, NativeHealthCheckExecutor,
    StoreError, run_health_work,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthCommandError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) state: Option<HealthRunStateResponse>,
}

impl HealthCommandError {
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: "conflict",
            message: message.into(),
            state: None,
        }
    }

    fn active(message: impl Into<String>) -> Self {
        Self {
            code: "health_run_active",
            message: message.into(),
            state: None,
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_request",
            message: message.into(),
            state: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthRunStateResponse {
    pub(crate) run_id: Uuid,
    pub(crate) state: HealthRunState,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthRunHandle {
    pub(crate) run_id: Uuid,
    pub(crate) scope: HealthRunScope,
    pub(crate) state: HealthRunState,
    pub(crate) revision: String,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) requested_installation_count: u64,
    pub(crate) scheduled_installation_count: u64,
    pub(crate) skipped_active_installation_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthMutation {
    pub(crate) run_id: Uuid,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HealthWorkspaceSnapshot {
    pub(crate) summary: HealthSummary,
    pub(crate) installations: Vec<InstallationHealthSnapshot>,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallationHealthDetail {
    pub(crate) snapshot: InstallationHealthSnapshot,
    pub(crate) history: Vec<HealthResult>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunInstallationHealthRequest {
    pub(crate) installation_id: String,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunAllHealthRequest {
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelHealthRequest {
    pub(crate) run_id: String,
    pub(crate) revision: String,
}

#[derive(Debug, Clone)]
struct ActiveHealthRun {
    state: HealthRunState,
    revision: String,
    cancellation: CancellationToken,
    installation_ids: HashSet<Uuid>,
}

#[derive(Debug)]
struct HealthRegistryState {
    workspace_revision: String,
    runs: HashMap<Uuid, ActiveHealthRun>,
    installation_owners: HashMap<Uuid, Uuid>,
}

#[derive(Debug)]
pub(crate) struct HealthRegistry {
    state: Mutex<HealthRegistryState>,
}

#[derive(Debug, Clone)]
pub(crate) struct AdmittedHealthRun {
    pub(crate) run_id: Uuid,
    pub(crate) handle: HealthRunHandle,
    pub(crate) cancellation: CancellationToken,
    pub(crate) scheduled_installation_ids: Vec<Uuid>,
}

impl HealthRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(HealthRegistryState {
                workspace_revision: Uuid::new_v4().to_string(),
                runs: HashMap::new(),
                installation_owners: HashMap::new(),
            }),
        }
    }

    pub(crate) fn workspace_revision(&self) -> String {
        self.state.lock().unwrap().workspace_revision.clone()
    }

    pub(crate) fn owns_installation(&self, installation_id: Uuid) -> bool {
        self.state
            .lock()
            .unwrap()
            .installation_owners
            .contains_key(&installation_id)
    }

    pub(crate) fn owner_of(&self, installation_id: Uuid) -> Option<Uuid> {
        self.state
            .lock()
            .unwrap()
            .installation_owners
            .get(&installation_id)
            .copied()
    }

    pub(crate) fn run_state(&self, run_id: Uuid) -> Option<HealthRunStateResponse> {
        let state = self.state.lock().unwrap();
        state.runs.get(&run_id).map(|run| HealthRunStateResponse {
            run_id,
            state: run.state.clone(),
            revision: run.revision.clone(),
        })
    }

    pub(crate) fn admit_single(
        &self,
        presented_revision: &str,
        installation_id: Uuid,
    ) -> Result<AdmittedHealthRun, HealthCommandError> {
        self.admit_single_with_persist(presented_revision, installation_id, || Ok(()))
    }

    pub(crate) fn admit_single_with_persist<F>(
        &self,
        presented_revision: &str,
        installation_id: Uuid,
        persist: F,
    ) -> Result<AdmittedHealthRun, HealthCommandError>
    where
        F: FnOnce() -> Result<(), HealthCommandError>,
    {
        self.admit_single_with_run_persist(presented_revision, installation_id, |_| persist())
    }

    pub(crate) fn admit_single_with_run_persist<F>(
        &self,
        presented_revision: &str,
        installation_id: Uuid,
        persist: F,
    ) -> Result<AdmittedHealthRun, HealthCommandError>
    where
        F: FnOnce(&HealthRun) -> Result<(), HealthCommandError>,
    {
        let mut state = self.state.lock().unwrap();
        if state.workspace_revision != presented_revision {
            return Err(HealthCommandError::conflict(
                "Health workspace revision is stale",
            ));
        }
        if state.installation_owners.contains_key(&installation_id) {
            return Err(HealthCommandError::active(
                "Health checks are already running for this installation",
            ));
        }

        let run_id = Uuid::new_v4();
        let run_revision = Uuid::new_v4().to_string();
        let started_at = Utc::now();
        let cancellation = CancellationToken::new();
        let installation_ids = HashSet::from([installation_id]);
        let run = HealthRun {
            id: run_id,
            scope: HealthRunScope::Installation,
            state: HealthRunState::Running,
            started_at: started_at.clone(),
            finished_at: None,
            requested_installation_count: 1,
            completed_installation_count: 0,
            revision: run_revision.clone(),
        };

        persist(&run)?;

        state.installation_owners.insert(installation_id, run_id);
        state.runs.insert(
            run_id,
            ActiveHealthRun {
                state: HealthRunState::Running,
                revision: run_revision.clone(),
                cancellation: cancellation.clone(),
                installation_ids,
            },
        );
        state.workspace_revision = Uuid::new_v4().to_string();

        Ok(AdmittedHealthRun {
            run_id,
            handle: HealthRunHandle {
                run_id,
                scope: HealthRunScope::Installation,
                state: HealthRunState::Running,
                revision: run_revision,
                started_at,
                requested_installation_count: 1,
                scheduled_installation_count: 1,
                skipped_active_installation_count: 0,
            },
            cancellation,
            scheduled_installation_ids: vec![installation_id],
        })
    }

    pub(crate) fn admit_all(
        &self,
        presented_revision: &str,
        installation_ids: Vec<Uuid>,
    ) -> Result<AdmittedHealthRun, HealthCommandError> {
        self.admit_all_with_run_persist(presented_revision, installation_ids, |_| Ok(()))
    }

    pub(crate) fn admit_all_with_run_persist<F>(
        &self,
        presented_revision: &str,
        installation_ids: Vec<Uuid>,
        persist: F,
    ) -> Result<AdmittedHealthRun, HealthCommandError>
    where
        F: FnOnce(&HealthRun) -> Result<(), HealthCommandError>,
    {
        let mut state = self.state.lock().unwrap();
        if state.workspace_revision != presented_revision {
            return Err(HealthCommandError::conflict(
                "Health workspace revision is stale",
            ));
        }

        let requested = installation_ids.len() as u64;
        let scheduled_installation_ids: Vec<Uuid> = installation_ids
            .into_iter()
            .filter(|id| !state.installation_owners.contains_key(id))
            .collect();
        let scheduled = scheduled_installation_ids.len() as u64;
        let skipped = requested.saturating_sub(scheduled);
        let run_id = Uuid::new_v4();
        let run_revision = Uuid::new_v4().to_string();
        let started_at = Utc::now();
        let cancellation = CancellationToken::new();
        let run_state = if scheduled == 0 {
            HealthRunState::Completed
        } else {
            HealthRunState::Running
        };
        let owned: HashSet<Uuid> = scheduled_installation_ids.iter().copied().collect();
        let run = HealthRun {
            id: run_id,
            scope: HealthRunScope::All,
            state: run_state.clone(),
            started_at: started_at.clone(),
            finished_at: if scheduled == 0 {
                Some(started_at.clone())
            } else {
                None
            },
            requested_installation_count: requested,
            completed_installation_count: 0,
            revision: run_revision.clone(),
        };

        persist(&run)?;

        for installation_id in &scheduled_installation_ids {
            state.installation_owners.insert(*installation_id, run_id);
        }
        state.runs.insert(
            run_id,
            ActiveHealthRun {
                state: run_state.clone(),
                revision: run_revision.clone(),
                cancellation: cancellation.clone(),
                installation_ids: owned,
            },
        );
        state.workspace_revision = Uuid::new_v4().to_string();

        Ok(AdmittedHealthRun {
            run_id,
            handle: HealthRunHandle {
                run_id,
                scope: HealthRunScope::All,
                state: run_state,
                revision: run_revision,
                started_at,
                requested_installation_count: requested,
                scheduled_installation_count: scheduled,
                skipped_active_installation_count: skipped,
            },
            cancellation,
            scheduled_installation_ids,
        })
    }

    pub(crate) fn cancel(
        &self,
        run_id: Uuid,
        presented_revision: &str,
    ) -> Result<HealthRunStateResponse, HealthCommandError> {
        let mut state = self.state.lock().unwrap();
        let run = state
            .runs
            .get_mut(&run_id)
            .ok_or_else(|| HealthCommandError::invalid_request("Unknown health run"))?;
        if run.revision != presented_revision {
            return Err(HealthCommandError::conflict("Health run revision is stale"));
        }

        if run.state == HealthRunState::Cancelling {
            return Ok(HealthRunStateResponse {
                run_id,
                state: run.state.clone(),
                revision: run.revision.clone(),
            });
        }
        if run.state != HealthRunState::Running {
            return Err(HealthCommandError::invalid_request(
                "Health run is not active",
            ));
        }

        run.state = HealthRunState::Cancelling;
        run.revision = Uuid::new_v4().to_string();
        run.cancellation.cancel();
        Ok(HealthRunStateResponse {
            run_id,
            state: run.state.clone(),
            revision: run.revision.clone(),
        })
    }

    pub(crate) fn cancel_all(&self) {
        let mut state = self.state.lock().unwrap();
        for run in state.runs.values_mut() {
            if run.state == HealthRunState::Running {
                run.state = HealthRunState::Cancelling;
            }
            run.cancellation.cancel();
        }
    }
    pub(crate) fn finish_registry_run(&self, run_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state.runs.remove(&run_id) {
            for installation_id in run.installation_ids {
                if state.installation_owners.get(&installation_id) == Some(&run_id) {
                    state.installation_owners.remove(&installation_id);
                }
            }
        }
    }
}

fn build_health_workspace_snapshot(
    store: &control_center_core::Store,
    revision: String,
    evaluated_at: DateTime<Utc>,
) -> Result<HealthWorkspaceSnapshot, StoreError> {
    let installations = store.installations()?;
    let mut snapshots = Vec::with_capacity(installations.len());
    for installation in installations {
        snapshots.push(store.installation_health(installation.id, evaluated_at.clone())?);
    }
    let summary = store.health_summary(evaluated_at)?;
    Ok(HealthWorkspaceSnapshot {
        summary,
        installations: snapshots,
        revision,
    })
}

fn build_installation_health_detail(
    store: &control_center_core::Store,
    installation_id: Uuid,
    evaluated_at: DateTime<Utc>,
) -> Result<InstallationHealthDetail, StoreError> {
    let snapshot = store.installation_health(installation_id, evaluated_at)?;
    let history = store.health_results_for_installation(installation_id, 100)?;
    Ok(InstallationHealthDetail { snapshot, history })
}

fn list_health_summary_from_state(
    state: &crate::scan_commands::AppState,
) -> Result<HealthWorkspaceSnapshot, HealthCommandError> {
    let revision = state.health.workspace_revision();
    let store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    build_health_workspace_snapshot(&store, revision, Utc::now()).map_err(map_health_store_error)
}

fn list_installation_health_from_state(
    installation_id: &str,
    state: &crate::scan_commands::AppState,
) -> Result<InstallationHealthDetail, HealthCommandError> {
    let installation_id = Uuid::parse_str(installation_id)
        .map_err(|_| HealthCommandError::invalid_request("Invalid installation id"))?;
    let store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    build_installation_health_detail(&store, installation_id, Utc::now())
        .map_err(map_health_store_error)
}

#[tauri::command]
pub(crate) fn list_health_summary(
    state: State<'_, crate::scan_commands::AppState>,
) -> Result<HealthWorkspaceSnapshot, HealthCommandError> {
    list_health_summary_from_state(&state)
}

#[tauri::command]
pub(crate) fn list_installation_health(
    installation_id: String,
    state: State<'_, crate::scan_commands::AppState>,
) -> Result<InstallationHealthDetail, HealthCommandError> {
    list_installation_health_from_state(&installation_id, &state)
}

#[tauri::command]
pub(crate) async fn run_health_checks(
    app: tauri::AppHandle,
    request: RunInstallationHealthRequest,
    state: State<'_, crate::scan_commands::AppState>,
) -> Result<HealthRunHandle, HealthCommandError> {
    let (admitted, work) = admit_single_health_run_from_state(&request, &state)?;
    let run = load_health_run_from_state(admitted.run_id, &state)?;
    let handle = admitted.handle.clone();
    spawn_health_run(app, admitted, run, vec![work]);
    Ok(handle)
}

#[tauri::command]
pub(crate) async fn run_all_health_checks(
    app: tauri::AppHandle,
    request: RunAllHealthRequest,
    state: State<'_, crate::scan_commands::AppState>,
) -> Result<HealthRunHandle, HealthCommandError> {
    let (admitted, work) = admit_all_health_runs_from_state(&request, &state)?;
    let handle = admitted.handle.clone();

    if admitted.handle.state == HealthRunState::Completed {
        state.health.finish_registry_run(admitted.run_id);
        return Ok(handle);
    }

    let run = load_health_run_from_state(admitted.run_id, &state)?;
    spawn_health_run(app, admitted, run, work);
    Ok(handle)
}

#[tauri::command]
pub(crate) fn cancel_health_run(
    request: CancelHealthRequest,
    state: State<'_, crate::scan_commands::AppState>,
) -> Result<HealthRunStateResponse, HealthCommandError> {
    let run_id = Uuid::parse_str(&request.run_id)
        .map_err(|_| HealthCommandError::invalid_request("Invalid health run id"))?;
    state.health.cancel(run_id, &request.revision)
}

fn admit_single_health_run_from_state(
    request: &RunInstallationHealthRequest,
    state: &crate::scan_commands::AppState,
) -> Result<(AdmittedHealthRun, InstallationHealthWork), HealthCommandError> {
    let installation_id = Uuid::parse_str(&request.installation_id)
        .map_err(|_| HealthCommandError::invalid_request("Invalid installation id"))?;

    let definitions = {
        let store = state.store.lock().map_err(|_| HealthCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".to_string(),
            state: None,
        })?;
        let installation_exists = store
            .installations()
            .map_err(map_health_store_error)?
            .into_iter()
            .any(|installation| installation.id == installation_id);
        if !installation_exists {
            return Err(HealthCommandError::invalid_request(
                "Installation is not available for health checks",
            ));
        }
        store
            .health_check_definitions(installation_id)
            .map_err(map_health_store_error)?
    };

    let work = InstallationHealthWork {
        installation_id,
        definitions,
    };

    let admitted =
        state
            .health
            .admit_single_with_run_persist(&request.revision, installation_id, |run| {
                let mut store = state.store.lock().map_err(|_| HealthCommandError {
                    code: "storage_integrity",
                    message: "Local storage is unavailable".to_string(),
                    state: None,
                })?;
                store.begin_health_run(run).map_err(map_health_store_error)
            })?;

    Ok((admitted, work))
}

fn admit_all_health_runs_from_state(
    request: &RunAllHealthRequest,
    state: &crate::scan_commands::AppState,
) -> Result<(AdmittedHealthRun, Vec<InstallationHealthWork>), HealthCommandError> {
    let all_work = {
        let store = state.store.lock().map_err(|_| HealthCommandError {
            code: "storage_integrity",
            message: "Local storage is unavailable".to_string(),
            state: None,
        })?;
        let installations = store.installations().map_err(map_health_store_error)?;
        let mut work = Vec::with_capacity(installations.len());
        for installation in installations {
            let definitions = store
                .health_check_definitions(installation.id)
                .map_err(map_health_store_error)?;
            work.push(InstallationHealthWork {
                installation_id: installation.id,
                definitions,
            });
        }
        work
    };

    let installation_ids = all_work.iter().map(|item| item.installation_id).collect();
    let admitted =
        state
            .health
            .admit_all_with_run_persist(&request.revision, installation_ids, |run| {
                let mut store = state.store.lock().map_err(|_| HealthCommandError {
                    code: "storage_integrity",
                    message: "Local storage is unavailable".to_string(),
                    state: None,
                })?;
                store.begin_health_run(run).map_err(map_health_store_error)
            })?;

    let scheduled: HashSet<Uuid> = admitted
        .scheduled_installation_ids
        .iter()
        .copied()
        .collect();
    let work = all_work
        .into_iter()
        .filter(|item| scheduled.contains(&item.installation_id))
        .collect();

    Ok((admitted, work))
}

fn load_health_run_from_state(
    run_id: Uuid,
    state: &crate::scan_commands::AppState,
) -> Result<HealthRun, HealthCommandError> {
    let store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    store.health_run(run_id).map_err(map_health_store_error)
}

fn persist_started_health_run_from_state(
    run: &HealthRun,
    state: &crate::scan_commands::AppState,
) -> Result<(), HealthCommandError> {
    let mut store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    store.update_health_run(run).map_err(map_health_store_error)
}

fn persist_health_result_from_state(
    result: &HealthResult,
    state: &crate::scan_commands::AppState,
) -> Result<HealthState, HealthCommandError> {
    let mut store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    store
        .record_health_result_and_recompute(result, result.observed_at)
        .map_err(map_health_store_error)
}

fn persist_terminal_health_run_from_state(
    run: &HealthRun,
    state: &crate::scan_commands::AppState,
) -> Result<(), HealthCommandError> {
    let mut store = state.store.lock().map_err(|_| HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".to_string(),
        state: None,
    })?;
    store.update_health_run(run).map_err(map_health_store_error)
}

pub(crate) fn map_health_store_error(_error: StoreError) -> HealthCommandError {
    HealthCommandError {
        code: "storage_integrity",
        message: "Local storage is unavailable".into(),
        state: None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum FrontendHealthEvent {
    Started {
        run_id: Uuid,
        scope: HealthRunScope,
        requested_installation_count: u64,
    },
    CheckCompleted {
        run_id: Uuid,
        result: control_center_core::HealthResult,
    },
    InstallationUpdated {
        run_id: Uuid,
        installation_id: Uuid,
        aggregate: control_center_core::HealthState,
    },
    Cancelled {
        run_id: Uuid,
        completed_installation_count: u64,
    },
    Completed {
        run_id: Uuid,
        completed_installation_count: u64,
    },
}

impl FrontendHealthEvent {
    pub(crate) fn run_id(&self) -> Uuid {
        match self {
            Self::Started { run_id, .. }
            | Self::CheckCompleted { run_id, .. }
            | Self::InstallationUpdated { run_id, .. }
            | Self::Cancelled { run_id, .. }
            | Self::Completed { run_id, .. } => *run_id,
        }
    }
}

trait HealthEventBridgePorts: Send + 'static {
    fn persist_started(&self, run: &control_center_core::HealthRun) -> Result<(), String>;
    fn persist_result(
        &self,
        result: &control_center_core::HealthResult,
    ) -> Result<control_center_core::HealthState, String>;
    fn persist_terminal(&self, run: &control_center_core::HealthRun) -> Result<(), String>;
    fn emit(&self, event: FrontendHealthEvent) -> Result<(), String>;
    fn finish_registry_run(&self, run_id: Uuid);
}

struct TauriHealthPorts(tauri::AppHandle);

impl HealthEventBridgePorts for TauriHealthPorts {
    fn persist_started(&self, run: &HealthRun) -> Result<(), String> {
        let state = self
            .0
            .try_state::<crate::scan_commands::AppState>()
            .ok_or_else(|| "Local storage is unavailable".to_string())?;
        persist_started_health_run_from_state(run, &state).map_err(|error| error.message)
    }

    fn persist_result(&self, result: &HealthResult) -> Result<HealthState, String> {
        let state = self
            .0
            .try_state::<crate::scan_commands::AppState>()
            .ok_or_else(|| "Local storage is unavailable".to_string())?;
        persist_health_result_from_state(result, &state).map_err(|error| error.message)
    }

    fn persist_terminal(&self, run: &HealthRun) -> Result<(), String> {
        let state = self
            .0
            .try_state::<crate::scan_commands::AppState>()
            .ok_or_else(|| "Local storage is unavailable".to_string())?;
        persist_terminal_health_run_from_state(run, &state).map_err(|error| error.message)
    }

    fn emit(&self, event: FrontendHealthEvent) -> Result<(), String> {
        self.0
            .emit("health:event", event)
            .map_err(|_| "Health event could not be emitted".to_string())
    }

    fn finish_registry_run(&self, run_id: Uuid) {
        if let Some(state) = self.0.try_state::<crate::scan_commands::AppState>() {
            state.health.finish_registry_run(run_id);
        }
    }
}

fn spawn_health_run(
    app: tauri::AppHandle,
    admitted: AdmittedHealthRun,
    run: HealthRun,
    work: Vec<InstallationHealthWork>,
) {
    let (sender, receiver) = tokio::sync::mpsc::channel(128);
    let events = HealthEventSink::new(sender);
    let cancellation = admitted.cancellation.clone();
    let run_id = admitted.run_id;
    let scope = run.scope.clone();

    tauri::async_runtime::spawn(async move {
        run_health_work(
            run_id,
            scope,
            work,
            Arc::new(NativeHealthCheckExecutor::default()),
            cancellation,
            events,
        )
        .await;
    });

    tauri::async_runtime::spawn(run_health_event_bridge(
        TauriHealthPorts(app),
        run,
        receiver,
    ));
}

async fn run_health_event_bridge<P: HealthEventBridgePorts>(
    ports: P,
    mut run: control_center_core::HealthRun,
    mut receiver: tokio::sync::mpsc::Receiver<control_center_core::HealthEvent>,
) {
    while let Some(event) = receiver.recv().await {
        match event {
            control_center_core::HealthEvent::Started { run_id, .. } => {
                if ports.persist_started(&run).is_err() {
                    continue;
                }
                let _ = ports.emit(FrontendHealthEvent::Started {
                    run_id,
                    scope: run.scope.clone(),
                    requested_installation_count: run.requested_installation_count,
                });
            }
            control_center_core::HealthEvent::CheckCompleted { run_id, result } => {
                let installation_id = result.installation_id;
                let aggregate = match ports.persist_result(&result) {
                    Ok(aggregate) => aggregate,
                    Err(_) => continue,
                };
                let _ = ports.emit(FrontendHealthEvent::CheckCompleted { run_id, result });
                let _ = ports.emit(FrontendHealthEvent::InstallationUpdated {
                    run_id,
                    installation_id,
                    aggregate,
                });
            }
            control_center_core::HealthEvent::Cancelled {
                run_id,
                completed_installation_count,
            } => {
                run.state = HealthRunState::Cancelled;
                run.completed_installation_count = completed_installation_count;
                run.finished_at = Some(Utc::now());
                if ports.persist_terminal(&run).is_ok() {
                    let _ = ports.emit(FrontendHealthEvent::Cancelled {
                        run_id,
                        completed_installation_count,
                    });
                    ports.finish_registry_run(run_id);
                }
                break;
            }
            control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count,
            } => {
                run.state = HealthRunState::Completed;
                run.completed_installation_count = completed_installation_count;
                run.finished_at = Some(Utc::now());
                if ports.persist_terminal(&run).is_ok() {
                    let _ = ports.emit(FrontendHealthEvent::Completed {
                        run_id,
                        completed_installation_count,
                    });
                    ports.finish_registry_run(run_id);
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_center_core::{HealthRunScope, Store};
    use uuid::Uuid;

    fn installation_ids(count: usize) -> Vec<Uuid> {
        (0..count).map(|_| Uuid::new_v4()).collect()
    }

    #[test]
    fn health_command_single_run_claims_only_requested_installation() {
        let registry = HealthRegistry::new();
        let ids = installation_ids(2);
        let revision = registry.workspace_revision();

        let admitted = registry.admit_single(&revision, ids[0]).unwrap();

        assert_eq!(admitted.handle.scope, HealthRunScope::Installation);
        assert_eq!(admitted.handle.requested_installation_count, 1);
        assert_eq!(admitted.handle.scheduled_installation_count, 1);
        assert_eq!(admitted.handle.skipped_active_installation_count, 0);
        assert!(registry.owns_installation(ids[0]));
        assert!(!registry.owns_installation(ids[1]));
    }

    #[test]
    fn health_command_duplicate_single_run_returns_active_error() {
        let registry = HealthRegistry::new();
        let installation_id = Uuid::new_v4();
        let revision = registry.workspace_revision();
        let first = registry.admit_single(&revision, installation_id).unwrap();
        let current_revision = registry.workspace_revision();

        let error = registry
            .admit_single(&current_revision, installation_id)
            .unwrap_err();

        assert_eq!(error.code, "health_run_active");
        assert_eq!(registry.workspace_revision(), current_revision);
        assert!(registry.owns_installation(installation_id));
        assert_eq!(registry.owner_of(installation_id), Some(first.run_id));
    }

    #[test]
    fn health_command_check_all_skips_installations_owned_by_other_runs() {
        let registry = HealthRegistry::new();
        let ids = installation_ids(3);
        let first_revision = registry.workspace_revision();
        registry.admit_single(&first_revision, ids[0]).unwrap();
        let all_revision = registry.workspace_revision();

        let admitted = registry.admit_all(&all_revision, ids.clone()).unwrap();

        assert_eq!(admitted.handle.scope, HealthRunScope::All);
        assert_eq!(admitted.handle.requested_installation_count, 3);
        assert_eq!(admitted.handle.scheduled_installation_count, 2);
        assert_eq!(admitted.handle.skipped_active_installation_count, 1);
        assert_eq!(admitted.scheduled_installation_ids, vec![ids[1], ids[2]]);
        assert!(registry.owns_installation(ids[0]));
        assert!(registry.owns_installation(ids[1]));
        assert!(registry.owns_installation(ids[2]));
    }

    #[test]
    fn health_command_check_all_respects_zero_inventory() {
        let registry = HealthRegistry::new();
        let revision = registry.workspace_revision();

        let admitted = registry.admit_all(&revision, Vec::new()).unwrap();

        assert_eq!(admitted.handle.scope, HealthRunScope::All);
        assert_eq!(admitted.handle.requested_installation_count, 0);
        assert_eq!(admitted.handle.scheduled_installation_count, 0);
        assert_eq!(admitted.handle.skipped_active_installation_count, 0);
        assert!(admitted.scheduled_installation_ids.is_empty());
    }

    #[test]
    fn health_command_check_all_all_active_completes_without_duplicate_work() {
        let registry = HealthRegistry::new();
        let ids = installation_ids(2);
        let first_revision = registry.workspace_revision();
        registry.admit_single(&first_revision, ids[0]).unwrap();
        let second_revision = registry.workspace_revision();
        registry.admit_single(&second_revision, ids[1]).unwrap();
        let all_revision = registry.workspace_revision();

        let admitted = registry.admit_all(&all_revision, ids.clone()).unwrap();

        assert_eq!(admitted.handle.requested_installation_count, 2);
        assert_eq!(admitted.handle.scheduled_installation_count, 0);
        assert_eq!(admitted.handle.skipped_active_installation_count, 2);
        assert!(admitted.scheduled_installation_ids.is_empty());
    }

    #[test]
    fn health_command_stale_workspace_revision_is_conflict_without_mutation() {
        let registry = HealthRegistry::new();
        let installation_id = Uuid::new_v4();
        let current_revision = registry.workspace_revision();

        let error = registry
            .admit_single("stale-workspace-revision", installation_id)
            .unwrap_err();

        assert_eq!(error.code, "conflict");
        assert_eq!(registry.workspace_revision(), current_revision);
        assert!(!registry.owns_installation(installation_id));
    }

    #[test]
    fn health_command_start_rotates_workspace_revision() {
        let registry = HealthRegistry::new();
        let first_revision = registry.workspace_revision();
        let installation_id = Uuid::new_v4();

        registry
            .admit_single(&first_revision, installation_id)
            .unwrap();
        let second_revision = registry.workspace_revision();
        assert_ne!(second_revision, first_revision);

        registry.finish_registry_run(registry.owner_of(installation_id).unwrap());
        let before_all = registry.workspace_revision();
        registry
            .admit_all(&before_all, vec![Uuid::new_v4()])
            .unwrap();
        let after_all = registry.workspace_revision();
        assert_ne!(after_all, before_all);
    }

    #[test]
    fn health_command_cancel_rotates_run_revision_once() {
        let registry = HealthRegistry::new();
        let installation_id = Uuid::new_v4();
        let workspace = registry.workspace_revision();
        let admitted = registry.admit_single(&workspace, installation_id).unwrap();
        let original_revision = admitted.handle.revision.clone();

        let first = registry
            .cancel(admitted.run_id, &original_revision)
            .unwrap();

        assert_eq!(first.state, control_center_core::HealthRunState::Cancelling);
        assert_ne!(first.revision, original_revision);
        assert!(admitted.cancellation.is_cancelled());

        let second = registry.cancel(admitted.run_id, &first.revision).unwrap();

        assert_eq!(
            second.state,
            control_center_core::HealthRunState::Cancelling
        );
        assert_eq!(second.revision, first.revision);
    }

    #[test]
    fn health_command_cancel_all_cancels_every_active_token() {
        let registry = HealthRegistry::new();
        let first_id = Uuid::new_v4();
        let first_revision = registry.workspace_revision();
        let first = registry.admit_single(&first_revision, first_id).unwrap();

        let second_id = Uuid::new_v4();
        let second_revision = registry.workspace_revision();
        let second = registry.admit_single(&second_revision, second_id).unwrap();

        registry.cancel_all();

        assert!(first.cancellation.is_cancelled());
        assert!(second.cancellation.is_cancelled());
    }
    #[test]
    fn health_command_stale_cancel_revision_is_conflict() {
        let registry = HealthRegistry::new();
        let installation_id = Uuid::new_v4();
        let workspace = registry.workspace_revision();
        let admitted = registry.admit_single(&workspace, installation_id).unwrap();

        let error = registry
            .cancel(admitted.run_id, "stale-run-revision")
            .unwrap_err();

        assert_eq!(error.code, "conflict");
        assert!(!admitted.cancellation.is_cancelled());
        assert_eq!(
            registry.run_state(admitted.run_id).unwrap().state,
            control_center_core::HealthRunState::Running
        );
        assert_eq!(
            registry.run_state(admitted.run_id).unwrap().revision,
            admitted.handle.revision
        );
    }

    #[test]
    fn health_command_scan_and_health_revisions_are_independent() {
        use crate::scan_commands::ScanRegistry;
        use control_center_core::ScanScope;

        let scans = ScanRegistry::new();
        let health = HealthRegistry::new();
        let initial_scan_revision = scans.workspace_revision();
        let initial_health_revision = health.workspace_revision();

        scans
            .start_with_revision(&initial_scan_revision, ScanScope::Quick)
            .unwrap();

        assert_ne!(scans.workspace_revision(), initial_scan_revision);
        assert_eq!(health.workspace_revision(), initial_health_revision);

        let scan_revision_after_scan_start = scans.workspace_revision();
        health
            .admit_single(&initial_health_revision, Uuid::new_v4())
            .unwrap();

        assert_ne!(health.workspace_revision(), initial_health_revision);
        assert_eq!(scans.workspace_revision(), scan_revision_after_scan_start);
    }

    fn sample_health_run(run_id: Uuid, requested: u64) -> control_center_core::HealthRun {
        control_center_core::HealthRun {
            id: run_id,
            scope: control_center_core::HealthRunScope::All,
            state: control_center_core::HealthRunState::Running,
            started_at: chrono::Utc::now(),
            finished_at: None,
            requested_installation_count: requested,
            completed_installation_count: 0,
            revision: "run-revision".into(),
        }
    }

    fn sample_health_result(
        run_id: Uuid,
        installation_id: Uuid,
    ) -> control_center_core::HealthResult {
        control_center_core::HealthResult {
            id: Uuid::new_v4(),
            run_id,
            installation_id,
            check_definition_id: Uuid::new_v4(),
            check_type: control_center_core::HealthCheckType::PathExists,
            required: true,
            state: control_center_core::HealthState::Green,
            result_code: "exists".into(),
            redacted_evidence: "present".into(),
            duration_ms: 1,
            observed_at: chrono::Utc::now(),
        }
    }

    struct RecordingHealthPorts {
        log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        fail_result: bool,
        fail_terminal: bool,
    }

    impl RecordingHealthPorts {
        fn new(log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>) -> Self {
            Self {
                log,
                fail_result: false,
                fail_terminal: false,
            }
        }
    }

    impl HealthEventBridgePorts for RecordingHealthPorts {
        fn persist_started(&self, _run: &control_center_core::HealthRun) -> Result<(), String> {
            self.log.lock().unwrap().push("persist_started");
            Ok(())
        }

        fn persist_result(
            &self,
            _result: &control_center_core::HealthResult,
        ) -> Result<control_center_core::HealthState, String> {
            self.log.lock().unwrap().push("persist_result");
            if self.fail_result {
                Err("storage failed".into())
            } else {
                Ok(control_center_core::HealthState::Green)
            }
        }

        fn persist_terminal(&self, _run: &control_center_core::HealthRun) -> Result<(), String> {
            self.log.lock().unwrap().push("persist_terminal");
            if self.fail_terminal {
                Err("storage failed".into())
            } else {
                Ok(())
            }
        }

        fn emit(&self, event: FrontendHealthEvent) -> Result<(), String> {
            let label = match event {
                FrontendHealthEvent::Started { .. } => "emit_started",
                FrontendHealthEvent::CheckCompleted { .. } => "emit_check_completed",
                FrontendHealthEvent::InstallationUpdated { .. } => "emit_installation_updated",
                FrontendHealthEvent::Cancelled { .. } => "emit_cancelled",
                FrontendHealthEvent::Completed { .. } => "emit_completed",
            };
            self.log.lock().unwrap().push(label);
            Ok(())
        }

        fn finish_registry_run(&self, _run_id: Uuid) {
            self.log.lock().unwrap().push("finish_registry_run");
        }
    }

    #[tokio::test]
    async fn health_command_persists_started_before_emitting_started() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingHealthPorts::new(log.clone());
        let run_id = Uuid::new_v4();
        let run = sample_health_run(run_id, 1);
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let bridge = tokio::spawn(run_health_event_bridge(ports, run, receiver));

        sender
            .send(control_center_core::HealthEvent::Started {
                run_id,
                scope: control_center_core::HealthRunScope::All,
                requested_installation_count: 1,
            })
            .await
            .unwrap();
        sender
            .send(control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count: 0,
            })
            .await
            .unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(&log[0..2], &["persist_started", "emit_started"]);
    }

    #[tokio::test]
    async fn health_command_persists_result_before_any_result_event() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingHealthPorts::new(log.clone());
        let run_id = Uuid::new_v4();
        let installation_id = Uuid::new_v4();
        let run = sample_health_run(run_id, 1);
        let result = sample_health_result(run_id, installation_id);
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let bridge = tokio::spawn(run_health_event_bridge(ports, run, receiver));

        sender
            .send(control_center_core::HealthEvent::CheckCompleted { run_id, result })
            .await
            .unwrap();
        sender
            .send(control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count: 1,
            })
            .await
            .unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(
            &log[0..3],
            &[
                "persist_result",
                "emit_check_completed",
                "emit_installation_updated"
            ]
        );
    }

    #[tokio::test]
    async fn health_command_persist_result_failure_emits_nothing_for_that_result() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        let mut ports = RecordingHealthPorts::new(log.clone());
        ports.fail_result = true;
        let run_id = Uuid::new_v4();
        let run = sample_health_run(run_id, 1);
        let result = sample_health_result(run_id, Uuid::new_v4());
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let bridge = tokio::spawn(run_health_event_bridge(ports, run, receiver));

        sender
            .send(control_center_core::HealthEvent::CheckCompleted { run_id, result })
            .await
            .unwrap();
        sender
            .send(control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count: 0,
            })
            .await
            .unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(log[0], "persist_result");
        assert!(!log.contains(&"emit_check_completed"));
        assert!(!log.contains(&"emit_installation_updated"));
    }

    #[tokio::test]
    async fn health_command_terminal_persist_precedes_terminal_event_and_cleanup() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingHealthPorts::new(log.clone());
        let run_id = Uuid::new_v4();
        let run = sample_health_run(run_id, 0);
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let bridge = tokio::spawn(run_health_event_bridge(ports, run, receiver));

        sender
            .send(control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count: 0,
            })
            .await
            .unwrap();
        drop(sender);
        bridge.await.unwrap();

        assert_eq!(
            &*log.lock().unwrap(),
            &["persist_terminal", "emit_completed", "finish_registry_run"]
        );
    }

    #[tokio::test]
    async fn health_command_terminal_persist_failure_does_not_claim_durable_completion() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        let mut ports = RecordingHealthPorts::new(log.clone());
        ports.fail_terminal = true;
        let run_id = Uuid::new_v4();
        let run = sample_health_run(run_id, 0);
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let bridge = tokio::spawn(run_health_event_bridge(ports, run, receiver));

        sender
            .send(control_center_core::HealthEvent::Completed {
                run_id,
                completed_installation_count: 0,
            })
            .await
            .unwrap();
        drop(sender);
        bridge.await.unwrap();

        assert_eq!(&*log.lock().unwrap(), &["persist_terminal"]);
    }

    #[test]
    fn health_command_frontend_events_include_run_id() {
        let run_id = Uuid::new_v4();
        let installation_id = Uuid::new_v4();
        let result = sample_health_result(run_id, installation_id);
        let events = vec![
            FrontendHealthEvent::Started {
                run_id,
                scope: control_center_core::HealthRunScope::All,
                requested_installation_count: 1,
            },
            FrontendHealthEvent::CheckCompleted { run_id, result },
            FrontendHealthEvent::InstallationUpdated {
                run_id,
                installation_id,
                aggregate: control_center_core::HealthState::Green,
            },
            FrontendHealthEvent::Cancelled {
                run_id,
                completed_installation_count: 0,
            },
            FrontendHealthEvent::Completed {
                run_id,
                completed_installation_count: 1,
            },
        ];

        for event in events {
            assert_eq!(event.run_id(), run_id);
        }
    }

    #[test]
    fn health_command_workspace_snapshot_uses_canonical_installations_and_health_revision() {
        let mut store = Store::in_memory().unwrap();
        let discovery = control_center_core::Discovery::unknown(
            "Health Snapshot Tool",
            "fixture",
            "health-snapshot".into(),
        );
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();

        let revision = "health-revision".to_string();
        let snapshot =
            build_health_workspace_snapshot(&store, revision.clone(), Utc::now()).unwrap();

        assert_eq!(snapshot.installations.len(), 1);
        assert_eq!(
            snapshot.installations[0].installation.source_discovery_id,
            discovery.id
        );
        assert_eq!(snapshot.summary.installation_count, 1);
        assert_eq!(snapshot.revision, revision);
    }

    #[test]
    fn health_command_installation_detail_returns_snapshot_and_history() {
        let mut store = Store::in_memory().unwrap();
        let discovery = control_center_core::Discovery::unknown(
            "Health Detail Tool",
            "fixture",
            "health-detail".into(),
        );
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();
        let installation = store.installations().unwrap().remove(0);

        let detail = build_installation_health_detail(&store, installation.id, Utc::now()).unwrap();

        assert_eq!(detail.snapshot.installation.id, installation.id);
        assert!(detail.history.is_empty());
    }

    #[test]
    fn health_command_single_admission_persists_generated_run_metadata() {
        let registry = HealthRegistry::new();
        let revision = registry.workspace_revision();
        let installation_id = Uuid::new_v4();
        let persisted = std::sync::Arc::new(Mutex::new(None::<HealthRun>));
        let captured = persisted.clone();

        let admitted = registry
            .admit_single_with_run_persist(&revision, installation_id, move |run| {
                *captured.lock().unwrap() = Some(run.clone());
                Ok(())
            })
            .unwrap();

        let run = persisted
            .lock()
            .unwrap()
            .clone()
            .expect("run was persisted");
        assert_eq!(run.id, admitted.run_id);
        assert_eq!(run.scope, HealthRunScope::Installation);
        assert_eq!(run.state, HealthRunState::Running);
        assert_eq!(run.started_at, admitted.handle.started_at);
        assert_eq!(run.requested_installation_count, 1);
        assert_eq!(run.completed_installation_count, 0);
        assert_eq!(run.revision, admitted.handle.revision);
    }

    #[test]
    fn health_command_summary_reads_state_with_health_revision() {
        let mut store = Store::in_memory().unwrap();
        let discovery = control_center_core::Discovery::unknown(
            "Health State Tool",
            "fixture",
            "health-state".into(),
        );
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();
        let state = crate::scan_commands::AppState::new(store);
        let expected_revision = state.health.workspace_revision();

        let snapshot = list_health_summary_from_state(&state).unwrap();

        assert_eq!(snapshot.summary.installation_count, 1);
        assert_eq!(snapshot.installations.len(), 1);
        assert_eq!(snapshot.revision, expected_revision);
    }

    #[test]
    fn health_command_installation_detail_reads_state_by_string_id() {
        let mut store = Store::in_memory().unwrap();
        let discovery = control_center_core::Discovery::unknown(
            "Health Detail State Tool",
            "fixture",
            "health-detail-state".into(),
        );
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();
        let installation = store.installations().unwrap().remove(0);
        let state = crate::scan_commands::AppState::new(store);

        let detail =
            list_installation_health_from_state(&installation.id.to_string(), &state).unwrap();

        assert_eq!(detail.snapshot.installation.id, installation.id);
        assert!(detail.history.is_empty());
    }

    #[test]
    fn health_command_installation_detail_rejects_invalid_id() {
        let state = crate::scan_commands::AppState::new(Store::in_memory().unwrap());

        let error = list_installation_health_from_state("not-a-uuid", &state).unwrap_err();

        assert_eq!(error.code, "invalid_request");
        assert_eq!(error.message, "Invalid installation id");
        assert!(error.state.is_none());
    }

    #[test]
    fn health_command_read_commands_are_exposed() {
        let _ = list_health_summary;
        let _ = list_installation_health;
    }

    #[test]
    fn health_command_single_run_admission_persists_run_and_builds_work() {
        let mut store = Store::in_memory().unwrap();
        let discovery = control_center_core::Discovery::unknown(
            "Health Run Tool",
            "fixture",
            "health-run".into(),
        );
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();
        let installation = store.installations().unwrap().remove(0);
        let state = crate::scan_commands::AppState::new(store);
        let request = RunInstallationHealthRequest {
            installation_id: installation.id.to_string(),
            revision: state.health.workspace_revision(),
        };

        let (admitted, work) = admit_single_health_run_from_state(&request, &state).unwrap();

        assert_eq!(admitted.scheduled_installation_ids, vec![installation.id]);
        assert_eq!(work.installation_id, installation.id);
        let persisted = state
            .store
            .lock()
            .unwrap()
            .health_run(admitted.run_id)
            .unwrap();
        assert_eq!(persisted.id, admitted.run_id);
        assert_eq!(persisted.state, HealthRunState::Running);
        assert_eq!(persisted.scope, HealthRunScope::Installation);
    }

    #[test]
    fn health_command_all_admission_persists_generated_run_metadata() {
        let registry = HealthRegistry::new();
        let revision = registry.workspace_revision();
        let installation_ids = vec![Uuid::new_v4(), Uuid::new_v4()];
        let persisted = std::sync::Arc::new(Mutex::new(None::<HealthRun>));
        let captured = persisted.clone();

        let admitted = registry
            .admit_all_with_run_persist(&revision, installation_ids.clone(), move |run| {
                *captured.lock().unwrap() = Some(run.clone());
                Ok(())
            })
            .unwrap();

        let run = persisted
            .lock()
            .unwrap()
            .clone()
            .expect("run was persisted");
        assert_eq!(run.id, admitted.run_id);
        assert_eq!(run.scope, HealthRunScope::All);
        assert_eq!(run.state, HealthRunState::Running);
        assert_eq!(run.started_at, admitted.handle.started_at);
        assert_eq!(run.requested_installation_count, 2);
        assert_eq!(run.completed_installation_count, 0);
        assert_eq!(run.revision, admitted.handle.revision);
        assert_eq!(admitted.scheduled_installation_ids, installation_ids);
    }

    #[test]
    fn health_command_all_run_admission_persists_run_and_builds_work() {
        let mut store = Store::in_memory().unwrap();
        for suffix in ["one", "two"] {
            let discovery = control_center_core::Discovery::unknown(
                format!("Health All {suffix}"),
                "fixture",
                format!("health-all-{suffix}"),
            );
            store.enqueue(&discovery).unwrap();
            store
                .review(discovery.id, control_center_core::ReviewDecision::Import)
                .unwrap();
        }
        let state = crate::scan_commands::AppState::new(store);
        let request = RunAllHealthRequest {
            revision: state.health.workspace_revision(),
        };

        let (admitted, work) = admit_all_health_runs_from_state(&request, &state).unwrap();

        assert_eq!(admitted.handle.requested_installation_count, 2);
        assert_eq!(admitted.handle.scheduled_installation_count, 2);
        assert_eq!(work.len(), 2);
        let persisted = state
            .store
            .lock()
            .unwrap()
            .health_run(admitted.run_id)
            .unwrap();
        assert_eq!(persisted.id, admitted.run_id);
        assert_eq!(persisted.scope, HealthRunScope::All);
        assert_eq!(persisted.state, HealthRunState::Running);
    }

    #[test]
    fn health_command_started_persistence_updates_existing_run() {
        let mut store = Store::in_memory().unwrap();
        let run = HealthRun {
            id: Uuid::new_v4(),
            scope: HealthRunScope::Installation,
            state: HealthRunState::Running,
            started_at: Utc::now(),
            finished_at: None,
            requested_installation_count: 1,
            completed_installation_count: 0,
            revision: Uuid::new_v4().to_string(),
        };
        store.begin_health_run(&run).unwrap();
        let state = crate::scan_commands::AppState::new(store);

        persist_started_health_run_from_state(&run, &state).unwrap();

        let persisted = state.store.lock().unwrap().health_run(run.id).unwrap();
        assert_eq!(persisted, run);
    }

    #[test]
    fn health_command_result_persistence_records_result_and_returns_aggregate() {
        let mut store = Store::in_memory().unwrap();
        let mut discovery = control_center_core::Discovery::unknown(
            "Health Result Tool",
            "fixture",
            "health-result".into(),
        );
        discovery
            .health_hints
            .push(control_center_core::HealthCheckHint {
                check_type: control_center_core::HealthCheckType::FileExists,
                required: true,
                expected_condition: control_center_core::HealthExpectedCondition::Exists,
                timeout_ms: 1_000,
                stale_after_seconds: None,
                target: control_center_core::HealthTarget::Path {
                    path: std::path::PathBuf::from(r"C:\Tools\health-result.exe"),
                },
            });
        store.enqueue(&discovery).unwrap();
        store
            .review(discovery.id, control_center_core::ReviewDecision::Import)
            .unwrap();
        let installation = store.installations().unwrap().remove(0);
        let definition = store
            .health_check_definitions(installation.id)
            .unwrap()
            .remove(0);
        let run = HealthRun {
            id: Uuid::new_v4(),
            scope: HealthRunScope::Installation,
            state: HealthRunState::Running,
            started_at: Utc::now(),
            finished_at: None,
            requested_installation_count: 1,
            completed_installation_count: 0,
            revision: Uuid::new_v4().to_string(),
        };
        store.begin_health_run(&run).unwrap();
        let result = HealthResult {
            id: Uuid::new_v4(),
            run_id: run.id,
            installation_id: installation.id,
            check_definition_id: definition.id,
            check_type: definition.check_type.clone(),
            required: definition.required,
            state: HealthState::Red,
            result_code: "file_missing".into(),
            redacted_evidence: "required file is missing".into(),
            duration_ms: 3,
            observed_at: Utc::now(),
        };
        let state = crate::scan_commands::AppState::new(store);

        let aggregate = persist_health_result_from_state(&result, &state).unwrap();

        assert_eq!(aggregate, HealthState::Red);
        let store = state.store.lock().unwrap();
        let history = store
            .health_results_for_installation(installation.id, 10)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, result.id);
        assert_eq!(
            store
                .installations()
                .unwrap()
                .into_iter()
                .find(|item| item.id == installation.id)
                .unwrap()
                .aggregate_health,
            HealthState::Red
        );
    }

    #[test]
    fn health_command_terminal_persistence_updates_existing_run() {
        let mut store = Store::in_memory().unwrap();
        let mut run = HealthRun {
            id: Uuid::new_v4(),
            scope: HealthRunScope::Installation,
            state: HealthRunState::Running,
            started_at: Utc::now(),
            finished_at: None,
            requested_installation_count: 1,
            completed_installation_count: 0,
            revision: Uuid::new_v4().to_string(),
        };
        store.begin_health_run(&run).unwrap();
        run.state = HealthRunState::Completed;
        run.completed_installation_count = 1;
        run.finished_at = Some(Utc::now());
        let state = crate::scan_commands::AppState::new(store);

        persist_terminal_health_run_from_state(&run, &state).unwrap();

        let persisted = state.store.lock().unwrap().health_run(run.id).unwrap();
        assert_eq!(persisted, run);
    }

    #[test]
    fn health_command_single_run_rejects_unknown_installation() {
        let state = crate::scan_commands::AppState::new(Store::in_memory().unwrap());
        let request = RunInstallationHealthRequest {
            installation_id: Uuid::new_v4().to_string(),
            revision: state.health.workspace_revision(),
        };

        let error = admit_single_health_run_from_state(&request, &state).unwrap_err();

        assert_eq!(error.code, "invalid_request");
        assert_eq!(
            error.message,
            "Installation is not available for health checks"
        );
    }

    #[test]
    fn health_command_tauri_ports_implement_health_bridge_contract() {
        fn assert_ports<T: HealthEventBridgePorts>() {}
        assert_ports::<TauriHealthPorts>();
    }

    #[test]
    fn health_command_loads_authoritative_persisted_run_for_bridge() {
        let mut store = Store::in_memory().unwrap();
        let run = HealthRun {
            id: Uuid::new_v4(),
            scope: HealthRunScope::All,
            state: HealthRunState::Running,
            started_at: Utc::now(),
            finished_at: None,
            requested_installation_count: 3,
            completed_installation_count: 0,
            revision: Uuid::new_v4().to_string(),
        };
        store.begin_health_run(&run).unwrap();
        let state = crate::scan_commands::AppState::new(store);

        let loaded = load_health_run_from_state(run.id, &state).unwrap();

        assert_eq!(loaded, run);
    }

    #[test]
    fn health_command_spawn_helper_has_expected_signature() {
        let _spawn: fn(
            tauri::AppHandle,
            AdmittedHealthRun,
            HealthRun,
            Vec<InstallationHealthWork>,
        ) = spawn_health_run;
    }

    #[test]
    fn health_command_run_single_is_exposed() {
        let _ = run_health_checks;
    }

    #[test]
    fn health_command_run_all_is_exposed() {
        let _ = run_all_health_checks;
    }

    #[test]
    fn health_command_cancel_is_exposed() {
        let _ = cancel_health_run;
    }

    #[test]
    fn health_command_storage_errors_are_redacted_for_ipc() {
        let internal = control_center_core::StoreError::StorageIntegrity(
            "secret-local-path=C:\\Users\\private\\health.db".into(),
        );

        let error = map_health_store_error(internal);

        assert_eq!(error.code, "storage_integrity");
        assert_eq!(error.message, "Local storage is unavailable");
        assert!(!error.message.contains("private"));
        assert!(error.state.is_none());
    }

    #[test]
    fn health_command_lock_order_does_not_deadlock_bootstrap_and_start() {
        use crate::scan_commands::AppState;
        use control_center_core::Store;

        let state = std::sync::Arc::new(AppState::new(Store::in_memory().unwrap()));
        let iterations = 200;
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let bootstrap_state = state.clone();
        let bootstrap_thread = std::thread::spawn(move || {
            for _ in 0..iterations {
                let _revision = bootstrap_state.health.workspace_revision();
                let _store = bootstrap_state.store.lock().unwrap();
            }
        });

        let start_state = state.clone();
        let start_thread = std::thread::spawn(move || {
            for _ in 0..iterations {
                let workspace = start_state.health.workspace_revision();
                let installation_id = Uuid::new_v4();
                let admitted = start_state
                    .health
                    .admit_single_with_persist(&workspace, installation_id, || {
                        let _store = start_state.store.lock().unwrap();
                        Ok(())
                    })
                    .unwrap();
                start_state.health.finish_registry_run(admitted.run_id);
            }
            let _ = done_tx.send(());
        });

        done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect(
                "bootstrap health reads and health admission deadlocked under concurrent access",
            );
        bootstrap_thread.join().unwrap();
        start_thread.join().unwrap();
    }
}
