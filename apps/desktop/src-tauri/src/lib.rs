use control_center_core::{Discovery, ReviewDecision, ScanEvent, Store, quick_scan};
use serde::Serialize;
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct AppState {
    store: Mutex<Store>,
    scans: Mutex<HashMap<Uuid, CancellationToken>>,
}

#[derive(Serialize)]
struct BootstrapState {
    mode: &'static str,
    pending: Vec<Discovery>,
    inventory: Vec<Discovery>,
}

#[tauri::command]
fn bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    let store = state.store.lock().map_err(|_| "storage lock poisoned")?;
    Ok(BootstrapState {
        mode: "desktop",
        pending: store.pending().map_err(|error| error.to_string())?,
        inventory: store.inventory().map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
fn review_discovery(
    id: String,
    decision: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|_| "invalid discovery id")?;
    let decision = match decision.as_str() {
        "import" => ReviewDecision::Import,
        "ignore" => ReviewDecision::Ignore,
        "unknown" => ReviewDecision::KeepUnknown,
        _ => return Err("invalid review decision".into()),
    };
    state
        .store
        .lock()
        .map_err(|_| "storage lock poisoned")?
        .review(id, decision)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_quick_scan(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let roots: Vec<PathBuf> = ["APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        return Err("Windows application data locations are unavailable".into());
    }
    let scan_id = Uuid::new_v4();
    let cancellation = CancellationToken::new();
    let mut scans = state.scans.lock().map_err(|_| "scan lock poisoned")?;
    if !scans.is_empty() {
        return Err("a quick scan is already active".into());
    }
    scans.insert(scan_id, cancellation.clone());
    drop(scans);
    let (sender, mut receiver) = mpsc::channel(128);
    let app_for_scan = app.clone();
    tauri::async_runtime::spawn(async move {
        quick_scan(roots, sender, cancellation).await;
    });
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let event = match &event {
                ScanEvent::Discovery { discovery } => {
                    let persisted = app_for_scan.try_state::<AppState>().is_some_and(|state| {
                        state
                            .store
                            .lock()
                            .is_ok_and(|mut store| store.enqueue(discovery).is_ok())
                    });
                    if persisted {
                        event
                    } else {
                        ScanEvent::ScannerFailed {
                            code: "storage_integrity".into(),
                            message: "A discovery could not be saved".into(),
                        }
                    }
                }
                _ => event,
            };
            if matches!(
                event,
                ScanEvent::Completed { .. }
                    | ScanEvent::Cancelled { .. }
                    | ScanEvent::Failed { .. }
            ) && let Some(state) = app_for_scan.try_state::<AppState>()
                && let Ok(mut scans) = state.scans.lock()
            {
                scans.remove(&scan_id);
            }
            let _ = app_for_scan.emit("scan:event", &event);
        }
        if let Some(state) = app_for_scan.try_state::<AppState>()
            && let Ok(mut scans) = state.scans.lock()
        {
            scans.remove(&scan_id);
        }
    });
    Ok(scan_id.to_string())
}

#[tauri::command]
fn cancel_scan(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|_| "invalid scan id")?;
    let scans = state.scans.lock().map_err(|_| "scan lock poisoned")?;
    let token = scans.get(&id).ok_or("scan not found")?;
    token.cancel();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("control-center.db"))
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(AppState {
                store: Mutex::new(store),
                scans: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_state,
            review_discovery,
            start_quick_scan,
            cancel_scan
        ])
        .run(tauri::generate_context!())
        .expect("AI Tool Control Center failed to start");
}
