pub mod action;
pub mod deep_scan;
mod deep_scan_windows;
pub mod model;
pub mod python_supervisor;
pub mod redaction;
pub mod scan;
pub mod scan_control;
pub mod storage;
pub mod windows;

pub use action::{ActionError, ActionPreview, ActionSource, CommandSpec};
pub use deep_scan::{
    DEEP_DIRECTORY_CONCURRENCY, DeepScanContext, DeepScanError, deep_scan, validate_deep_roots,
};
pub use model::*;
pub use scan::{PythonRootError, QuickScanContext, quick_scan};
pub use scan_control::{PauseGate, ScanEvent, ScanEventSink, ScanLifecycleState, ScanScope};
pub use storage::{ReviewDecision, Store, StoreError};

pub mod health;
pub use health::{
    HealthProbeIo, HealthProbeOutcome, MAX_CONFIG_BYTES, NativeHealthProbeIo, aggregate_health,
    execute_health_check, is_result_stale,
};
