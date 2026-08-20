//! Generic scan command layer for the desktop shell.
//!
//! Responsibilities are kept in four clearly separated sections:
//!   1. wire request/response types and the stable command error,
//!   2. the active-scan registry (revision checks and state transitions),
//!   3. the persistence-before-notification bridge, and
//!   4. the Tauri commands (including the native folder picker).
//!
//! Everything in section 2 is deliberately free of Tauri types so the
//! revision/conflict state machine is unit-testable without a window.

use crate::runtime_root;
use chrono::{DateTime, Utc};
use control_center_core::{
    Discovery, PauseGate, QuickScanContext, ScanEvent, ScanEventSink, ScanLifecycleState,
    ScanScope, Store, deep_scan, deep_scan::DeepScanContext, deep_scan::DeepScanError, quick_scan,
    validate_deep_roots,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 1. Wire types
// ---------------------------------------------------------------------------

/// Stable, machine-readable command failure. The `code` set is closed:
/// `conflict`, `scan_active`, `scan_not_found`, `no_roots_selected`,
/// `network_consent_required`, `storage_integrity`, `invalid_request`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
    /// Present only for `storage_integrity` on a pause/resume whose in-memory
    /// state (and revision) already changed before persistence failed. Lets
    /// the frontend adopt the new revision instead of being permanently
    /// stuck retrying with a now-stale one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ScanState>,
}

impl CommandError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            state: None,
        }
    }

    fn with_state(mut self, state: ScanState) -> Self {
        self.state = Some(state);
        self
    }

    pub(crate) fn conflict() -> Self {
        Self::new(
            "conflict",
            "The scan control state changed; refresh before retrying",
        )
    }

    pub(crate) fn scan_active() -> Self {
        Self::new("scan_active", "A scan is already running")
    }

    pub(crate) fn scan_not_found() -> Self {
        Self::new("scan_not_found", "That scan is no longer active")
    }

    pub(crate) fn no_roots_selected() -> Self {
        Self::new("no_roots_selected", "Select at least one folder to scan")
    }

    pub(crate) fn network_consent_required() -> Self {
        Self::new(
            "network_consent_required",
            "A selected root is on the network and requires explicit consent for this scan",
        )
    }

    pub(crate) fn storage_integrity(message: impl Into<String>) -> Self {
        Self::new("storage_integrity", message)
    }

    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub mode: ScanScope,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub follow_reparse_points: bool,
    #[serde(default)]
    pub network_consent: bool,
    pub revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMutationRequest {
    pub scan_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHandle {
    pub scan_id: String,
    pub scope: ScanScope,
    pub state: ScanLifecycleState,
    pub revision: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    pub scan_id: String,
    pub state: ScanLifecycleState,
    pub revision: String,
}

// ---------------------------------------------------------------------------
// 2. Active-scan registry (Tauri-free, unit-testable)
// ---------------------------------------------------------------------------

struct ActiveScan {
    cancellation: CancellationToken,
    pause_gate: PauseGate,
    /// Recorded for the run so the registry is self-describing; the wire
    /// copies live on the `ScanHandle` already returned to the frontend.
    #[allow(dead_code)]
    scope: ScanScope,
    state: ScanLifecycleState,
    revision: String,
    #[allow(dead_code)]
    started_at: DateTime<Utc>,
}

/// Result of a successful admission: the caller owns the token and gate it
/// must hand to the scan runner.
pub(crate) struct AdmittedScan {
    pub(crate) handle: ScanHandle,
    pub(crate) cancellation: CancellationToken,
    pub(crate) pause_gate: PauseGate,
}

// `PauseGate` is not `Debug`, so this is written out by hand to keep
// `Result<AdmittedScan, _>::unwrap_err()` usable in tests.
impl std::fmt::Debug for AdmittedScan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdmittedScan")
            .field("handle", &self.handle)
            .field("cancelled", &self.cancellation.is_cancelled())
            .field("paused", &self.pause_gate.is_paused())
            .finish()
    }
}

/// Result of a lifecycle transition. `changed` is `false` for a safe no-op
/// (already in the requested state at the presented, current revision), in
/// which case the revision was deliberately not rotated.
#[derive(Debug)]
pub(crate) struct TransitionOutcome {
    pub(crate) state: ScanState,
    pub(crate) changed: bool,
}

fn new_revision() -> String {
    Uuid::new_v4().to_string()
}

pub(crate) struct ScanRegistry {
    scans: Mutex<HashMap<Uuid, ActiveScan>>,
    workspace_revision: Mutex<String>,
}

impl ScanRegistry {
    pub(crate) fn new() -> Self {
        Self {
            scans: Mutex::new(HashMap::new()),
            workspace_revision: Mutex::new(new_revision()),
        }
    }

    /// The workspace scan-control revision handed out by `bootstrap_state`.
    pub(crate) fn workspace_revision(&self) -> String {
        self.workspace_revision
            .lock()
            .map(|revision| revision.clone())
            .unwrap_or_default()
    }

    /// Admits a new scan.
    ///
    /// The whole sequence runs under the scans lock so a concurrent second
    /// start cannot slip between the "no active scan" check and the insert:
    ///   1. compare the presented workspace revision,
    ///   2. reject a second active scan,
    ///   3. generate ids, per-scan revision and root cancellation token,
    ///   4. persist the `scan_runs` row as `running` (caller-supplied),
    ///   5. rotate the workspace revision,
    ///   6. insert the `ActiveScan`.
    ///
    /// `persist` is synchronous by construction so no lock is held across an
    /// await point.
    pub(crate) fn admit<P>(
        &self,
        presented_revision: &str,
        scope: ScanScope,
        persist: P,
    ) -> Result<AdmittedScan, CommandError>
    where
        P: FnOnce(Uuid, DateTime<Utc>) -> Result<(), CommandError>,
    {
        let mut workspace = self
            .workspace_revision
            .lock()
            .map_err(|_| CommandError::storage_integrity("Scan control state is unavailable"))?;
        let mut scans = self
            .scans
            .lock()
            .map_err(|_| CommandError::storage_integrity("Scan control state is unavailable"))?;

        if presented_revision != workspace.as_str() {
            return Err(CommandError::conflict());
        }
        if !scans.is_empty() {
            return Err(CommandError::scan_active());
        }

        let scan_id = Uuid::new_v4();
        let revision = new_revision();
        let started_at = Utc::now();
        let cancellation = CancellationToken::new();
        let pause_gate = PauseGate::default();

        persist(scan_id, started_at)?;

        *workspace = new_revision();

        scans.insert(
            scan_id,
            ActiveScan {
                cancellation: cancellation.clone(),
                pause_gate: pause_gate.clone(),
                scope,
                state: ScanLifecycleState::Running,
                revision: revision.clone(),
                started_at,
            },
        );

        Ok(AdmittedScan {
            handle: ScanHandle {
                scan_id: scan_id.to_string(),
                scope,
                state: ScanLifecycleState::Running,
                revision,
                started_at,
            },
            cancellation,
            pause_gate,
        })
    }

    /// Convenience wrapper used by tests: admit without persistence.
    #[cfg(test)]
    pub(crate) fn start_with_revision(
        &self,
        presented_revision: &str,
        scope: ScanScope,
    ) -> Result<AdmittedScan, CommandError> {
        self.admit(presented_revision, scope, |_, _| Ok(()))
    }

    fn parse_id(scan_id: &str) -> Result<Uuid, CommandError> {
        Uuid::parse_str(scan_id).map_err(|_| CommandError::invalid_request("Invalid scan id"))
    }

    /// Shared body for pause/resume/cancel: parse the id, compare the
    /// per-scan revision, no-op when already in the target state, otherwise
    /// apply `apply`, rotate the revision and record the new state.
    fn transition(
        &self,
        scan_id: String,
        presented_revision: &str,
        target: ScanLifecycleState,
        apply: impl FnOnce(&ActiveScan),
    ) -> Result<TransitionOutcome, CommandError> {
        let id = Self::parse_id(&scan_id)?;
        let mut scans = self
            .scans
            .lock()
            .map_err(|_| CommandError::storage_integrity("Scan control state is unavailable"))?;
        let active = scans
            .get_mut(&id)
            .ok_or_else(CommandError::scan_not_found)?;

        if presented_revision != active.revision.as_str() {
            return Err(CommandError::conflict());
        }

        if active.state == target {
            return Ok(TransitionOutcome {
                state: ScanState {
                    scan_id,
                    state: active.state,
                    revision: active.revision.clone(),
                },
                changed: false,
            });
        }

        apply(active);
        active.revision = new_revision();
        active.state = target;

        Ok(TransitionOutcome {
            state: ScanState {
                scan_id,
                state: active.state,
                revision: active.revision.clone(),
            },
            changed: true,
        })
    }

    pub(crate) fn pause(
        &self,
        scan_id: String,
        presented_revision: &str,
    ) -> Result<TransitionOutcome, CommandError> {
        self.transition(
            scan_id,
            presented_revision,
            ScanLifecycleState::Paused,
            |active| {
                active.pause_gate.pause();
            },
        )
    }

    pub(crate) fn resume(
        &self,
        scan_id: String,
        presented_revision: &str,
    ) -> Result<TransitionOutcome, CommandError> {
        self.transition(
            scan_id,
            presented_revision,
            ScanLifecycleState::Running,
            |active| {
                active.pause_gate.resume();
            },
        )
    }

    /// Cancellation has precedence over a pause: the gate is opened so
    /// checkpointed work can observe the cancelled token immediately.
    pub(crate) fn cancel(
        &self,
        scan_id: String,
        presented_revision: &str,
    ) -> Result<TransitionOutcome, CommandError> {
        self.transition(
            scan_id,
            presented_revision,
            ScanLifecycleState::Cancelled,
            |active| {
                active.pause_gate.resume();
                active.cancellation.cancel();
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn state_of(&self, scan_id: &str) -> Option<ScanState> {
        let id = Uuid::parse_str(scan_id).ok()?;
        let scans = self.scans.lock().ok()?;
        let active = scans.get(&id)?;
        Some(ScanState {
            scan_id: scan_id.to_string(),
            state: active.state,
            revision: active.revision.clone(),
        })
    }

    fn remove(&self, scan_id: Uuid) {
        if let Ok(mut scans) = self.scans.lock() {
            scans.remove(&scan_id);
        }
    }

    /// Synchronous, non-blocking shutdown path: cancel every active token
    /// and open every pause gate. Never waits for a scan to settle.
    pub(crate) fn cancel_all(&self) {
        if let Ok(mut scans) = self.scans.lock() {
            for active in scans.values_mut() {
                active.pause_gate.resume();
                active.cancellation.cancel();
                active.state = ScanLifecycleState::Cancelled;
            }
        }
    }
}

pub(crate) struct AppState {
    pub(crate) store: Mutex<Store>,
    pub(crate) scans: ScanRegistry,
}

impl AppState {
    pub(crate) fn new(store: Store) -> Self {
        Self {
            store: Mutex::new(store),
            scans: ScanRegistry::new(),
        }
    }

    fn with_store<T>(
        &self,
        action: impl FnOnce(&mut Store) -> Result<T, control_center_core::StoreError>,
    ) -> Result<T, CommandError> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| CommandError::storage_integrity("Local storage is unavailable"))?;
        action(&mut store).map_err(|error| CommandError::storage_integrity(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// 3. Persistence-before-notification bridge
// ---------------------------------------------------------------------------

const STORAGE_INTEGRITY_MESSAGE: &str = "Scan results could not be saved locally";

fn emit(app: &tauri::AppHandle, event: &ScanEvent) {
    let _ = app.emit("scan:event", event);
}

fn persist_discovery(
    app: &tauri::AppHandle,
    scan_id: Uuid,
    discovery: &Discovery,
) -> Result<(), CommandError> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| CommandError::storage_integrity("Local storage is unavailable"))?;
    state.with_store(|store| store.enqueue_for_scan(scan_id, discovery))
}

fn persist_scan_error(
    app: &tauri::AppHandle,
    scan_id: Uuid,
    scanner_id: &str,
    code: &str,
    message: &str,
) -> Result<(), CommandError> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| CommandError::storage_integrity("Local storage is unavailable"))?;
    state
        .with_store(|store| store.record_scan_error(scan_id, scanner_id, code, message, Utc::now()))
}

fn persist_terminal(
    app: &tauri::AppHandle,
    scan_id: Uuid,
    state: ScanLifecycleState,
    failure_count: u64,
) -> Result<(), CommandError> {
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| CommandError::storage_integrity("Local storage is unavailable"))?;
    app_state.with_store(|store| store.finish_scan(scan_id, state, Utc::now(), failure_count))
}

fn terminal_state_of(event: &ScanEvent) -> Option<ScanLifecycleState> {
    match event {
        ScanEvent::Cancelled { .. } => Some(ScanLifecycleState::Cancelled),
        ScanEvent::Completed { .. } => Some(ScanLifecycleState::Completed),
        ScanEvent::Failed { .. } => Some(ScanLifecycleState::Failed),
        _ => None,
    }
}

/// Rebuilds a terminal event with the bridge's own storage failures folded
/// into its failure count.
fn with_added_failures(event: ScanEvent, extra: u64) -> ScanEvent {
    match event {
        ScanEvent::Cancelled {
            visited,
            discovered,
            failure_count,
            duration_ms,
        } => ScanEvent::Cancelled {
            visited,
            discovered,
            failure_count: failure_count + extra,
            duration_ms,
        },
        ScanEvent::Completed {
            visited,
            discovered,
            failure_count,
            duration_ms,
        } => ScanEvent::Completed {
            visited,
            discovered,
            failure_count: failure_count + extra,
            duration_ms,
        },
        ScanEvent::Failed {
            code,
            message,
            failure_count,
            duration_ms,
        } => ScanEvent::Failed {
            code,
            message,
            failure_count: failure_count + extra,
            duration_ms,
        },
        other => other,
    }
}

fn duration_ms_of(event: &ScanEvent) -> u64 {
    match event {
        ScanEvent::Cancelled { duration_ms, .. }
        | ScanEvent::Completed { duration_ms, .. }
        | ScanEvent::Failed { duration_ms, .. } => *duration_ms,
        _ => 0,
    }
}

/// The bridge's dependencies, abstracted so the persistence-before-
/// notification ordering can be exercised without a real Tauri window.
/// `TauriPorts` is the production implementation; tests use a
/// call-order-recording fake.
trait EventBridgePorts: Send + 'static {
    fn persist_discovery(&self, scan_id: Uuid, discovery: &Discovery) -> Result<(), CommandError>;
    fn persist_scan_error(
        &self,
        scan_id: Uuid,
        scanner_id: &str,
        code: &str,
        message: &str,
    ) -> Result<(), CommandError>;
    fn persist_terminal(
        &self,
        scan_id: Uuid,
        state: ScanLifecycleState,
        failure_count: u64,
    ) -> Result<(), CommandError>;
    fn emit(&self, event: &ScanEvent);
    fn remove_scan(&self, scan_id: Uuid);
}

struct TauriPorts(tauri::AppHandle);

impl EventBridgePorts for TauriPorts {
    fn persist_discovery(&self, scan_id: Uuid, discovery: &Discovery) -> Result<(), CommandError> {
        persist_discovery(&self.0, scan_id, discovery)
    }

    fn persist_scan_error(
        &self,
        scan_id: Uuid,
        scanner_id: &str,
        code: &str,
        message: &str,
    ) -> Result<(), CommandError> {
        persist_scan_error(&self.0, scan_id, scanner_id, code, message)
    }

    fn persist_terminal(
        &self,
        scan_id: Uuid,
        state: ScanLifecycleState,
        failure_count: u64,
    ) -> Result<(), CommandError> {
        persist_terminal(&self.0, scan_id, state, failure_count)
    }

    fn emit(&self, event: &ScanEvent) {
        emit(&self.0, event)
    }

    fn remove_scan(&self, scan_id: Uuid) {
        if let Some(state) = self.0.try_state::<AppState>() {
            state.scans.remove(scan_id);
        }
    }
}

/// Drains the runner's event channel, persisting every event's durable
/// consequence *before* announcing it to the frontend. Generic over
/// [`EventBridgePorts`] so the ordering invariant is unit-testable without a
/// Tauri window (see `persistence_before_notification_*` tests below).
async fn run_event_bridge<P: EventBridgePorts>(
    ports: P,
    scan_id: Uuid,
    mut receiver: mpsc::Receiver<ScanEvent>,
) {
    let mut storage_failures: u64 = 0;
    let mut terminal_seen = false;

    while let Some(event) = receiver.recv().await {
        match &event {
            ScanEvent::Discovery { discovery } => {
                if ports.persist_discovery(scan_id, discovery).is_err() {
                    // The discovery is not announced: there is no durable
                    // record to back it. Record the integrity failure
                    // instead and count it toward the run's failures.
                    storage_failures += 1;
                    let scanner_id = discovery.source_scanner.clone();
                    let _ = ports.persist_scan_error(
                        scan_id,
                        &scanner_id,
                        "storage_integrity",
                        STORAGE_INTEGRITY_MESSAGE,
                    );
                    ports.emit(&ScanEvent::ScannerFailed {
                        scanner_id,
                        code: "storage_integrity".into(),
                        message: STORAGE_INTEGRITY_MESSAGE.into(),
                    });
                    continue;
                }
                ports.emit(&event);
            }
            ScanEvent::ScannerFailed {
                scanner_id,
                code,
                message,
            } => {
                if ports
                    .persist_scan_error(scan_id, scanner_id, code, message)
                    .is_err()
                {
                    storage_failures += 1;
                }
                // The failure itself is still news even if recording it
                // did not survive, so it is always announced.
                ports.emit(&event);
            }
            // Paused/Resumed are owned by their commands, which persist
            // and emit them directly. The runner never originates them.
            ScanEvent::Paused | ScanEvent::Resumed => {}
            _ => {
                if let Some(state) = terminal_state_of(&event) {
                    terminal_seen = true;
                    let event = with_added_failures(event.clone(), storage_failures);
                    let failure_count = match &event {
                        ScanEvent::Cancelled { failure_count, .. }
                        | ScanEvent::Completed { failure_count, .. }
                        | ScanEvent::Failed { failure_count, .. } => *failure_count,
                        _ => storage_failures,
                    };

                    if ports
                        .persist_terminal(scan_id, state, failure_count)
                        .is_err()
                    {
                        // There is no valid terminal record to announce as
                        // successful, so the run is reported as failed.
                        ports.emit(&ScanEvent::Failed {
                            code: "storage_integrity".into(),
                            message: STORAGE_INTEGRITY_MESSAGE.into(),
                            failure_count: failure_count + 1,
                            duration_ms: duration_ms_of(&event),
                        });
                    } else {
                        ports.emit(&event);
                    }

                    ports.remove_scan(scan_id);
                    break;
                }
                ports.emit(&event);
            }
        }
    }

    // The channel closed without ever producing a terminal event: the
    // spawned runner (quick_scan/deep_scan) panicked, was aborted, or was
    // otherwise dropped without settling. Without this fallback the
    // scan_runs row would stay `running` forever and the frontend would
    // never receive a terminal event to release its active-scan state.
    if !terminal_seen {
        let event = ScanEvent::Failed {
            code: "runner_stopped".into(),
            message: "The scan process stopped unexpectedly".into(),
            failure_count: storage_failures,
            duration_ms: 0,
        };
        let _ = ports.persist_terminal(scan_id, ScanLifecycleState::Failed, storage_failures);
        ports.emit(&event);
    }

    ports.remove_scan(scan_id);
}

fn spawn_event_bridge(app: tauri::AppHandle, scan_id: Uuid, receiver: mpsc::Receiver<ScanEvent>) {
    tauri::async_runtime::spawn(run_event_bridge(TauriPorts(app), scan_id, receiver));
}

// ---------------------------------------------------------------------------
// 4. Commands
// ---------------------------------------------------------------------------

/// Native multi-folder selection. The picker is driven from Rust through the
/// dialog plugin, so JavaScript never invokes it and no `dialog:*` capability
/// permission is required. Closing the picker yields an empty vector.
#[tauri::command]
pub(crate) async fn pick_scan_roots(app: tauri::AppHandle) -> Result<Vec<String>, CommandError> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();

    app.dialog()
        .file()
        .set_title("Select Deep Scan roots")
        .pick_folders(move |paths| {
            let _ = tx.send(paths);
        });

    let selected = rx
        .await
        .map_err(|_| CommandError::invalid_request("Folder picker closed unexpectedly"))?;

    Ok(selected
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub(crate) async fn start_scan(
    app: tauri::AppHandle,
    request: ScanRequest,
    state: State<'_, AppState>,
) -> Result<ScanHandle, CommandError> {
    // 3. Validate mode-specific request fields, and 4. reject Deep with no
    //    roots, before anything is generated or persisted.
    let deep_context = match request.mode {
        ScanScope::Quick => {
            if !request.roots.is_empty() || request.follow_reparse_points || request.network_consent
            {
                return Err(CommandError::invalid_request(
                    "Quick Scan does not accept roots, reparse-point or network options",
                ));
            }
            None
        }
        ScanScope::Deep => {
            if request.roots.is_empty() {
                return Err(CommandError::no_roots_selected());
            }
            let roots: Vec<PathBuf> = request.roots.iter().map(PathBuf::from).collect();

            // 5. Core root validation (including unconfirmed network roots)
            //    runs synchronously, here, before scan id generation,
            //    persistence or spawning: an unconfirmed network root must
            //    be rejected as a command error, not admitted and failed
            //    later inside the spawned `deep_scan` task.
            if let Err(error) = validate_deep_roots(&roots, request.network_consent) {
                return Err(match error {
                    DeepScanError::NoRootsSelected => CommandError::no_roots_selected(),
                    DeepScanError::NetworkConsentRequired => {
                        CommandError::network_consent_required()
                    }
                    DeepScanError::RootUnavailable => {
                        CommandError::invalid_request(error.to_string())
                    }
                });
            }

            Some(DeepScanContext {
                roots,
                follow_reparse_points: request.follow_reparse_points,
                network_consent: request.network_consent,
            })
        }
    };

    let quick_context = match request.mode {
        ScanScope::Quick => {
            let roots: Vec<PathBuf> = ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from)
                .collect();
            if roots.is_empty() {
                return Err(CommandError::no_roots_selected());
            }
            Some(QuickScanContext {
                roots,
                python_app_root: runtime_root::resolve_python_app_root(),
            })
        }
        ScanScope::Deep => None,
    };

    // 1., 2., 6.-9.: workspace revision check, single-active-scan rejection,
    // id/revision/token generation, `scan_runs` persistence, workspace
    // revision rotation and registry insert, all atomically.
    let scope = request.mode;
    let admitted = state
        .scans
        .admit(&request.revision, scope, |scan_id, started_at| {
            state.with_store(|store| store.begin_scan(scan_id, scope, started_at))
        })?;

    let scan_id = Uuid::parse_str(&admitted.handle.scan_id)
        .map_err(|_| CommandError::invalid_request("Invalid scan id"))?;

    // 10. Bounded event channel and sink.
    let (sender, receiver) = mpsc::channel(128);
    let events = ScanEventSink::new(sender);

    // 11. Spawn the runner. Each mode emits its own `Started` event with its
    //     own scanner count.
    let cancellation = admitted.cancellation.clone();
    let pause_gate = admitted.pause_gate.clone();
    match (quick_context, deep_context) {
        (Some(context), _) => {
            tauri::async_runtime::spawn(async move {
                quick_scan(context, events, pause_gate, cancellation).await;
            });
        }
        (_, Some(context)) => {
            tauri::async_runtime::spawn(async move {
                deep_scan(context, events, cancellation, pause_gate).await;
            });
        }
        _ => unreachable!("one scan mode context is always built"),
    }

    // 12. Persistence/event bridge.
    spawn_event_bridge(app, scan_id, receiver);

    // 13.
    Ok(admitted.handle)
}

fn persist_lifecycle(
    state: &AppState,
    scan_id: &str,
    lifecycle: ScanLifecycleState,
) -> Result<(), CommandError> {
    let id =
        Uuid::parse_str(scan_id).map_err(|_| CommandError::invalid_request("Invalid scan id"))?;
    state.with_store(|store| store.set_scan_state(id, lifecycle))
}

#[tauri::command]
pub(crate) fn pause_scan(
    app: tauri::AppHandle,
    request: ScanMutationRequest,
    state: State<'_, AppState>,
) -> Result<ScanState, CommandError> {
    let outcome = state.scans.pause(request.scan_id, &request.revision)?;
    if outcome.changed {
        persist_lifecycle(&state, &outcome.state.scan_id, ScanLifecycleState::Paused)
            .map_err(|error| error.with_state(outcome.state.clone()))?;
        emit(&app, &ScanEvent::Paused);
    }
    Ok(outcome.state)
}

#[tauri::command]
pub(crate) fn resume_scan(
    app: tauri::AppHandle,
    request: ScanMutationRequest,
    state: State<'_, AppState>,
) -> Result<ScanState, CommandError> {
    let outcome = state.scans.resume(request.scan_id, &request.revision)?;
    if outcome.changed {
        persist_lifecycle(&state, &outcome.state.scan_id, ScanLifecycleState::Running)
            .map_err(|error| error.with_state(outcome.state.clone()))?;
        emit(&app, &ScanEvent::Resumed);
    }
    Ok(outcome.state)
}

/// Requests cancellation. Deliberately does *not* persist the terminal
/// `scan_runs` state or emit `cancelled`: the runner bridge does both once
/// bounded work and owned processes have settled.
#[tauri::command]
pub(crate) fn cancel_scan(
    request: ScanMutationRequest,
    state: State<'_, AppState>,
) -> Result<ScanState, CommandError> {
    Ok(state
        .scans
        .cancel(request.scan_id, &request.revision)?
        .state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_center_core::{ScanLifecycleState, ScanScope};

    #[test]
    fn start_with_stale_workspace_revision_conflicts() {
        let registry = ScanRegistry::new();

        assert_eq!(
            registry
                .start_with_revision("stale", ScanScope::Quick)
                .unwrap_err()
                .code,
            "conflict"
        );
    }

    #[test]
    fn pause_rotates_revision_and_repeat_pause_is_a_safe_no_op() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        let admitted = registry
            .start_with_revision(&workspace, ScanScope::Quick)
            .unwrap();
        let current_revision = admitted.handle.revision.clone();

        let paused = registry
            .pause(admitted.handle.scan_id.clone(), &current_revision)
            .unwrap();
        assert_eq!(paused.state.state, ScanLifecycleState::Paused);
        assert!(paused.changed);
        assert_ne!(paused.state.revision, current_revision);

        let no_op = registry
            .pause(admitted.handle.scan_id.clone(), &paused.state.revision)
            .unwrap();
        assert!(!no_op.changed);
        assert_eq!(no_op.state.revision, paused.state.revision);
    }

    #[test]
    fn resume_with_pre_pause_revision_conflicts_without_mutating_state() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        let admitted = registry
            .start_with_revision(&workspace, ScanScope::Quick)
            .unwrap();
        let pre_pause_revision = admitted.handle.revision.clone();

        let paused = registry
            .pause(admitted.handle.scan_id.clone(), &pre_pause_revision)
            .unwrap();

        let error = registry
            .resume(admitted.handle.scan_id.clone(), &pre_pause_revision)
            .unwrap_err();
        assert_eq!(error.code, "conflict");

        // The stale attempt must not have resumed the gate or rotated again.
        assert!(admitted.pause_gate.is_paused());
        assert_eq!(
            registry
                .state_of(&admitted.handle.scan_id)
                .unwrap()
                .revision,
            paused.state.revision
        );
    }

    #[test]
    fn cancel_while_paused_cancels_the_token() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        let admitted = registry
            .start_with_revision(&workspace, ScanScope::Quick)
            .unwrap();

        let paused = registry
            .pause(admitted.handle.scan_id.clone(), &admitted.handle.revision)
            .unwrap();
        assert!(admitted.pause_gate.is_paused());

        let cancelled = registry
            .cancel(admitted.handle.scan_id.clone(), &paused.state.revision)
            .unwrap();

        assert_eq!(cancelled.state.state, ScanLifecycleState::Cancelled);
        assert!(cancelled.changed);
        assert_ne!(cancelled.state.revision, paused.state.revision);
        assert!(admitted.cancellation.is_cancelled());
    }

    #[test]
    fn repeat_cancel_at_current_revision_is_a_safe_no_op() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        let admitted = registry
            .start_with_revision(&workspace, ScanScope::Quick)
            .unwrap();

        let cancelled = registry
            .cancel(admitted.handle.scan_id.clone(), &admitted.handle.revision)
            .unwrap();
        let repeat = registry
            .cancel(admitted.handle.scan_id.clone(), &cancelled.state.revision)
            .unwrap();

        assert!(!repeat.changed);
        assert_eq!(repeat.state.revision, cancelled.state.revision);
    }

    #[test]
    fn a_second_start_while_one_is_active_is_rejected() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        registry
            .start_with_revision(&workspace, ScanScope::Quick)
            .unwrap();

        let rotated = registry.workspace_revision();
        assert_ne!(rotated, workspace);

        assert_eq!(
            registry
                .start_with_revision(&rotated, ScanScope::Deep)
                .unwrap_err()
                .code,
            "scan_active"
        );
    }

    #[test]
    fn unknown_scan_id_is_scan_not_found_and_malformed_id_is_invalid_request() {
        let registry = ScanRegistry::new();

        assert_eq!(
            registry
                .pause(uuid::Uuid::new_v4().to_string(), "whatever")
                .unwrap_err()
                .code,
            "scan_not_found"
        );
        assert_eq!(
            registry
                .cancel("not-a-uuid".into(), "whatever")
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }

    #[test]
    fn cancel_all_cancels_every_active_token_and_opens_pause_gates() {
        let registry = ScanRegistry::new();
        let workspace = registry.workspace_revision();
        let admitted = registry
            .start_with_revision(&workspace, ScanScope::Deep)
            .unwrap();
        registry
            .pause(admitted.handle.scan_id.clone(), &admitted.handle.revision)
            .unwrap();

        registry.cancel_all();

        assert!(admitted.cancellation.is_cancelled());
        assert!(!admitted.pause_gate.is_paused());
    }

    // -----------------------------------------------------------------
    // persistence-before-notification ordering (spawn_event_bridge)
    // -----------------------------------------------------------------

    /// Call-order-recording fake standing in for the Tauri-backed ports:
    /// every persist/emit call appends a label instead of touching a real
    /// store or window, so the exact call sequence can be asserted.
    struct RecordingPorts {
        log: std::sync::Arc<Mutex<Vec<&'static str>>>,
    }

    impl EventBridgePorts for RecordingPorts {
        fn persist_discovery(
            &self,
            _scan_id: Uuid,
            _discovery: &Discovery,
        ) -> Result<(), CommandError> {
            self.log.lock().unwrap().push("persist_discovery");
            Ok(())
        }

        fn persist_scan_error(
            &self,
            _scan_id: Uuid,
            _scanner_id: &str,
            _code: &str,
            _message: &str,
        ) -> Result<(), CommandError> {
            self.log.lock().unwrap().push("persist_scan_error");
            Ok(())
        }

        fn persist_terminal(
            &self,
            _scan_id: Uuid,
            _state: ScanLifecycleState,
            _failure_count: u64,
        ) -> Result<(), CommandError> {
            self.log.lock().unwrap().push("persist_terminal");
            Ok(())
        }

        fn emit(&self, event: &ScanEvent) {
            let label = match event {
                ScanEvent::Discovery { .. } => "emit_discovery",
                ScanEvent::ScannerFailed { .. } => "emit_scanner_failed",
                ScanEvent::Cancelled { .. }
                | ScanEvent::Completed { .. }
                | ScanEvent::Failed { .. } => "emit_terminal",
                _ => "emit_other",
            };
            self.log.lock().unwrap().push(label);
        }

        fn remove_scan(&self, _scan_id: Uuid) {}
    }

    fn sample_discovery() -> Discovery {
        Discovery::unknown("sample", "filesystem.deep", "fingerprint-1".into())
    }

    fn sample_completed() -> ScanEvent {
        ScanEvent::Completed {
            visited: 1,
            discovered: 1,
            failure_count: 0,
            duration_ms: 1,
        }
    }

    #[tokio::test]
    async fn persistence_before_notification_discovery() {
        let log: std::sync::Arc<Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingPorts { log: log.clone() };
        let (sender, receiver) = mpsc::channel(8);
        let scan_id = Uuid::new_v4();

        let bridge = tokio::spawn(run_event_bridge(ports, scan_id, receiver));

        sender
            .send(ScanEvent::Discovery {
                discovery: sample_discovery(),
            })
            .await
            .unwrap();
        // A terminal event ends the bridge loop so the test can join it.
        sender.send(sample_completed()).await.unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(&log[0..2], &["persist_discovery", "emit_discovery"]);
        assert_eq!(&log[2..4], &["persist_terminal", "emit_terminal"]);
    }

    #[tokio::test]
    async fn persistence_before_notification_scanner_failed() {
        let log: std::sync::Arc<Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingPorts { log: log.clone() };
        let (sender, receiver) = mpsc::channel(8);
        let scan_id = Uuid::new_v4();

        let bridge = tokio::spawn(run_event_bridge(ports, scan_id, receiver));

        sender
            .send(ScanEvent::ScannerFailed {
                scanner_id: "filesystem.deep".into(),
                code: "permission_denied".into(),
                message: "denied".into(),
            })
            .await
            .unwrap();
        sender.send(sample_completed()).await.unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(&log[0..2], &["persist_scan_error", "emit_scanner_failed"]);
        assert_eq!(&log[2..4], &["persist_terminal", "emit_terminal"]);
    }

    #[tokio::test]
    async fn persistence_before_notification_terminal() {
        let log: std::sync::Arc<Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingPorts { log: log.clone() };
        let (sender, receiver) = mpsc::channel(8);
        let scan_id = Uuid::new_v4();

        let bridge = tokio::spawn(run_event_bridge(ports, scan_id, receiver));

        sender.send(sample_completed()).await.unwrap();
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(&log[..], &["persist_terminal", "emit_terminal"]);
    }

    #[tokio::test]
    async fn runner_exiting_without_a_terminal_event_still_persists_and_emits_one() {
        let log: std::sync::Arc<Mutex<Vec<&'static str>>> = Default::default();
        let ports = RecordingPorts { log: log.clone() };
        let (sender, receiver) = mpsc::channel(8);
        let scan_id = Uuid::new_v4();

        let bridge = tokio::spawn(run_event_bridge(ports, scan_id, receiver));

        // Simulate the spawned runner dying without ever emitting a terminal
        // event (e.g. a top-level panic): the channel just closes.
        drop(sender);
        bridge.await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(
            &log[..],
            &["persist_terminal", "emit_terminal"],
            "a runner that exits without a terminal event must still leave the scan in a durable, announced terminal state"
        );
    }
}
