mod runtime_root;
mod scan_commands;

use control_center_core::{Discovery, ReviewDecision, Store};
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
    /// The workspace scan-control revision that `start_scan` must present.
    scan_revision: String,
}

#[tauri::command]
fn bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, CommandError> {
    let store = state
        .store
        .lock()
        .map_err(|_| CommandError::storage_integrity("Local storage is unavailable"))?;
    Ok(BootstrapState {
        mode: "desktop",
        pending: store
            .pending()
            .map_err(|error| CommandError::storage_integrity(error.to_string()))?,
        inventory: store
            .inventory()
            .map_err(|error| CommandError::storage_integrity(error.to_string()))?,
        scan_revision: state.scans.workspace_revision(),
    })
}

#[tauri::command]
fn review_discovery(
    id: String,
    decision: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let id = Uuid::parse_str(&id)
        .map_err(|_| CommandError::invalid_request("Invalid discovery id"))?;
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
        .map_err(|error| CommandError::storage_integrity(error.to_string()))
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
            // Closing the window cancels every active scan token. This is
            // synchronous and non-blocking: it never waits for a scan to
            // settle. The Python Job Object stays kill-on-close, so dropping
            // the owned supervisor closes descendants even if runtime
            // shutdown follows immediately.
            if matches!(event, WindowEvent::CloseRequested { .. })
                && let Some(state) = window.try_state::<AppState>()
            {
                state.scans.cancel_all();
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
        ])
        .run(tauri::generate_context!())
        .expect("AI Tool Control Center failed to start");
}
