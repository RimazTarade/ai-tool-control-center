pub mod action;
mod deep_scan_windows;
pub mod model;
pub mod python_supervisor;
pub mod redaction;
pub mod scan;
pub mod scan_control;
pub mod storage;
pub mod windows;

pub use action::{ActionError, ActionPreview, ActionSource, CommandSpec};
pub use model::*;
pub use scan::{PythonRootError, QuickScanContext, quick_scan};
pub use scan_control::{PauseGate, ScanEvent, ScanEventSink, ScanLifecycleState, ScanScope};
pub use storage::{ReviewDecision, Store, StoreError};
