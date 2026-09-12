#[cfg(all(test, target_os = "windows"))]
#[link(name = "resource")]
unsafe extern "C" {}

mod health_commands;
mod runtime_root;
mod scan_commands;

use control_center_core::{
    CanonicalCapability, Discovery, InstallationInstance, ReviewDecision, Store,
};
use health_commands::{
    cancel_health_run, list_health_summary, list_installation_health, run_all_health_checks,
    run_health_checks,
};
use scan_commands::{
    AppState, CommandError, cancel_scan, pause_scan, pick_scan_roots, resume_scan, start_scan,
};
use serde::Serialize;
use std::fs;
use tauri::{Manager, State, WindowEvent};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapState {
    mode: &'static str,
    pending: Vec<Discovery>,
    inventory: Vec<Discovery>,
    capabilities: Vec<CanonicalCapability>,
    installations: Vec<InstallationInstance>,
    /// The workspace scan-control revision that `start_scan` must present.
    scan_revision: String,
    health_revision: String,
}

#[tauri::command]
fn bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, CommandError> {
    build_bootstrap_state(&state)
}

fn map_store_error_for_ipc(_error: control_center_core::StoreError) -> CommandError {
    CommandError::storage_integrity("Local storage is unavailable")
}

fn build_bootstrap_state(state: &AppState) -> Result<BootstrapState, CommandError> {
    let scan_revision = state.scans.workspace_revision();
    let health_revision = state.health.workspace_revision();
    let store = state
        .store
        .lock()
        .map_err(|_| CommandError::storage_integrity("Local storage is unavailable"))?;
    Ok(BootstrapState {
        mode: "desktop",
        pending: store.pending().map_err(map_store_error_for_ipc)?,
        inventory: store.inventory().map_err(map_store_error_for_ipc)?,
        capabilities: store.capabilities().map_err(map_store_error_for_ipc)?,
        installations: store.installations().map_err(map_store_error_for_ipc)?,
        scan_revision,
        health_revision,
    })
}

#[tauri::command]
fn review_discovery(
    id: String,
    decision: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let id =
        Uuid::parse_str(&id).map_err(|_| CommandError::invalid_request("Invalid discovery id"))?;
    let decision = match decision.as_str() {
        "import" => ReviewDecision::Import,
        "ignore" => ReviewDecision::Ignore,
        "unknown" => ReviewDecision::KeepUnknown,
        _ => return Err(CommandError::invalid_request("Invalid review decision")),
    };
    state
        .store
        .lock()
        .map_err(|_| CommandError::storage_integrity("Local storage is unavailable"))?
        .review(id, decision)
        .map_err(map_store_error_for_ipc)
}

fn cancel_active_work(state: &AppState) {
    state.scans.cancel_all();
    state.health.cancel_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lib_store_errors_are_redacted_for_ipc() {
        let internal = control_center_core::StoreError::StorageIntegrity(
            "secret-local-path=C:\\Users\\private\\control-center.db".into(),
        );

        let error = map_store_error_for_ipc(internal);

        assert_eq!(error.code, "storage_integrity");
        assert_eq!(error.message, "Local storage is unavailable");
        assert!(!error.message.contains("private"));
    }
    #[test]
    fn lib_bootstrap_exposes_canonical_inventory_and_health_revision() {
        let state = AppState::new(Store::in_memory().unwrap());
        let expected_health_revision = state.health.workspace_revision();

        let bootstrap = build_bootstrap_state(&state).unwrap();

        assert_eq!(bootstrap.health_revision, expected_health_revision);
        assert!(bootstrap.capabilities.is_empty());
        assert!(bootstrap.installations.is_empty());
    }
    #[test]
    fn lib_registers_all_health_commands() {
        let source = include_str!("lib.rs");
        let handler = source
            .rsplit_once(".invoke_handler(tauri::generate_handler![")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split("])").next())
            .expect("generate_handler block");

        for command in [
            "list_health_summary",
            "list_installation_health",
            "run_health_checks",
            "run_all_health_checks",
            "cancel_health_run",
        ] {
            assert!(
                handler.contains(command),
                "missing health command: {command}"
            );
        }
    }
    #[test]
    fn lib_shutdown_cancels_active_health_tokens() {
        let state = AppState::new(Store::in_memory().unwrap());
        let installation_id = Uuid::new_v4();
        let revision = state.health.workspace_revision();
        let admitted = state
            .health
            .admit_single(&revision, installation_id)
            .unwrap();

        cancel_active_work(&state);

        assert!(admitted.cancellation.is_cancelled());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("control-center.db"))
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(AppState::new(store));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window cancels every active scan and health token. This is
            // synchronous and non-blocking: it never waits for active work to
            // settle. The Python Job Object stays kill-on-close, so dropping
            // the owned supervisor closes descendants even if runtime
            // shutdown follows immediately.
            if matches!(event, WindowEvent::CloseRequested { .. })
                && let Some(state) = window.try_state::<AppState>()
            {
                cancel_active_work(&state);
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_state,
            review_discovery,
            pick_scan_roots,
            start_scan,
            pause_scan,
            resume_scan,
            cancel_scan,
            list_health_summary,
            list_installation_health,
            run_health_checks,
            run_all_health_checks,
            cancel_health_run,
        ])
        .run(tauri::generate_context!())
        .expect("AI Tool Control Center failed to start");
}
