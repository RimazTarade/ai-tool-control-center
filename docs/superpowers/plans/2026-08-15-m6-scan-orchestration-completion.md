# Milestone 6 Scan Orchestration Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Milestone 6 against the approved master plan by replacing Quick-only scan control with a generic cancellable and pausable scan coordinator, adding safe Deep Scan traversal, persisting scan settlement, exposing native root selection and proving responsive frontend behavior with Playwright.

**Architecture:** Keep the existing eight Quick Scan adapters, but move shared lifecycle control into `scan_control.rs` and make Quick and Deep feed one bounded `ScanEvent` channel. Add a dedicated `deep_scan.rs` traversal engine and `deep_scan_windows.rs` Windows filesystem policy layer, move Tauri scan state and commands into `scan_commands.rs` and keep the React frontend generic around one active scan. Persist discoveries, scanner failures and lifecycle settlement before frontend notification, while allowing only progress events to be dropped under backpressure.

**Tech Stack:** Rust 1.96, Tokio 1.53.1, Tauri 2.11.5, tauri-plugin-dialog 2.7.2, rusqlite 0.40.1, React 19.2.8, TypeScript 7.0.2, Vitest 4.1.10, Playwright 1.62.1 and pnpm 10.33.2.

## Global Constraints

- Preserve the existing eight Quick Scan adapters and the completed Python Quick Scan integration.
- Replace the public `start_quick_scan` command and frontend wrapper with generic `start_scan(ScanRequest)`. Do not retain a compatibility alias.
- Quick Scan runs at most 4 scanner jobs concurrently.
- Deep Scan runs only `filesystem.deep` and performs at most 8 directory reads concurrently. It does not rerun the eight Quick scanners.
- Both modes use cooperative pause. A bounded operation already running may settle, but no new checkpointed work starts while paused.
- Cancellation must remain effective while paused.
- Cloud-only, offline and recall-on-access placeholders are skipped. Milestone 6 never hydrates them.
- Symbolic links and junctions are not followed by default. Following them is an explicit per-scan option.
- When reparse-point following is enabled, stable filesystem identity must prevent cycles.
- UNC roots and mapped network drives require explicit confirmation for each Deep Scan. No blanket consent is stored.
- Raw selected paths may exist only in the user-selected request and in backend traversal state. `scan_runs.scope` stores only `quick` or `deep`, and scan events never expose raw paths.
- Progress `current_location` is redacted. Use the form `Selected root N · depth D`, never a filesystem path.
- Use one Tauri event channel named `scan:event`.
- Wire event `kind` values are `started`, `progress`, `discovery`, `scanner_failed`, `paused`, `resumed`, `cancelled`, `completed` and `failed`.
- The coordinator queue is bounded. Progress may be coalesced or dropped when full. Discoveries, scanner failures, lifecycle changes and terminal events must never be silently dropped.
- Persist each discovery before its `discovery` event.
- Persist each scanner failure to `scan_errors` before its `scanner_failed` event.
- Persist scan lifecycle state before `paused` and `resumed` events.
- Persist final `scan_runs` state before `cancelled`, `completed` or `failed`.
- `scan.started` reports public scope plus scanner count. Quick reports 8 and Deep reports 1.
- Terminal events include visited count, discovered count, failure count and duration in milliseconds where applicable.
- Cancellation has precedence over a child operation settling at the same time.
- Python process cancellation first closes child stdin, waits up to 2 seconds, then closes the Windows Job Object if the process tree did not exit. A forced termination records `scanner_terminated`.
- Closing the Tauri main window cancels all active scan tokens. Owned Job Objects retain kill-on-close behavior.
- `bootstrap_state()` returns a workspace scan-control revision.
- `start_scan` must present the current workspace revision. An accepted start rotates the workspace revision.
- `ScanHandle.revision` is a separate per-scan revision used by pause, resume and cancel.
- A real pause, resume or cancel state change rotates the per-scan revision.
- A safe no-op command with the current revision returns the current state without rotating it.
- A stale workspace or per-scan revision returns the stable `conflict` command error without mutating state.
- After a terminal event, the frontend refreshes `bootstrap_state()` before another scan so it obtains the rotated workspace revision.
- Keep generated Tauri schemas generated. Do not hand-edit `apps/desktop/src-tauri/gen/schemas/*`.
- Keep `apps/desktop/src-tauri/capabilities/default.json` at `core:default`; the folder picker is called from Rust through the plugin rather than exposed as a frontend plugin command.
- No new paid service, API or remote dependency is introduced.

## File Responsibility Map

- Create `engine/rust/control-center-core/src/scan_control.rs`
  - Shared lifecycle enums, pause gate, event contract and bounded event sink.
- Modify `engine/rust/control-center-core/src/scan.rs`
  - Existing Quick Scan adapters and Quick orchestration only.
- Create `engine/rust/control-center-core/src/deep_scan_windows.rs`
  - Windows root classification, placeholder/reparse metadata and stable directory identity.
- Create `engine/rust/control-center-core/src/deep_scan.rs`
  - Explicit-root Deep Scan queue, exclusions, cycle prevention and bounded directory concurrency.
- Modify `engine/rust/control-center-core/src/python_supervisor.rs`
  - Distinguish cooperative cancellation from forced Job Object termination.
- Modify `engine/rust/control-center-core/src/storage.rs`
  - Add scan run/error persistence and scan ownership for newly discovered rows.
- Modify `engine/rust/control-center-core/src/lib.rs`
  - Export the M6 core contracts.
- Create `apps/desktop/src-tauri/src/scan_commands.rs`
  - Tauri request/response types, active-scan registry, revision checks, persistence bridge, native picker and scan commands.
- Modify `apps/desktop/src-tauri/src/lib.rs`
  - Register the dialog plugin, M6 commands and window-close cancellation.
- Modify `apps/desktop/src-tauri/Cargo.toml`
  - Add `tauri-plugin-dialog = "2.7.2"`.
- Modify `apps/frontend/src/model.ts`
  - Generic scan request, state and event types.
- Modify `apps/frontend/src/api.ts`
  - Generic scan commands and the single `scan:event` subscription.
- Modify `apps/frontend/src/App.tsx`
  - Scan dialog, selected roots, scan bar and lifecycle controls.
- Modify `apps/frontend/src/styles.css`
  - Compact modal and persistent scan-bar styling.
- Modify `apps/frontend/src/App.desktop.test.tsx`
  - Desktop command and lifecycle interaction tests.
- Modify `apps/frontend/src/App.test.tsx`
  - Browser demo scan lifecycle tests.
- Modify `apps/frontend/package.json`
  - Add Playwright and `test:e2e`.
- Modify `pnpm-lock.yaml`
  - Lock Playwright 1.62.1.
- Create `apps/frontend/playwright.config.ts`
  - Browser E2E configuration and Vite web server.
- Create `apps/frontend/e2e/scan-orchestration.spec.ts`
  - Slow scan responsiveness, warning isolation, pause/resume/cancel and restart coverage.

---

### Task 1: Shared scan lifecycle, pause gate and bounded event sink

**Files:**
- Create: `engine/rust/control-center-core/src/scan_control.rs`
- Modify: `engine/rust/control-center-core/src/scan.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline `#[cfg(test)]` modules in `scan_control.rs` and `scan.rs`

**Interfaces:**
- Consumes: `tokio::sync::mpsc`, `tokio::sync::Notify`, `tokio_util::sync::CancellationToken`, existing `Discovery`.
- Produces:
  - `pub enum ScanScope { Quick, Deep }`
  - `pub enum ScanLifecycleState { Running, Paused, Cancelled, Completed, Failed }`
  - `pub struct PauseGate`
  - `PauseGate::pause(&self) -> bool`
  - `PauseGate::resume(&self) -> bool`
  - `PauseGate::is_paused(&self) -> bool`
  - `PauseGate::checkpoint(&self, cancellation: &CancellationToken) -> bool`
  - `pub enum ScanEvent`
  - `pub struct ScanEventSink`
  - `ScanEventSink::progress(&self, event: ScanEvent)`
  - `ScanEventSink::critical(&self, event: ScanEvent) -> Result<(), mpsc::error::SendError<ScanEvent>>`
  - `ScanEventSink::scanner_failed(&self, scanner_id: impl Into<String>, code: impl Into<String>, message: impl Into<String>)`
  - `ScanEventSink::failure_count(&self) -> u64`
  - `pub const QUICK_SCAN_CONCURRENCY: usize = 4`

- [ ] **Step 1: Add failing pause-gate tests**

Create `scan_control.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn pause_gate_blocks_until_resume() {
        let gate = PauseGate::default();
        let cancellation = CancellationToken::new();

        assert!(gate.pause());

        let waiter = tokio::spawn({
            let gate = gate.clone();
            let cancellation = cancellation.clone();
            async move { gate.checkpoint(&cancellation).await }
        });

        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        assert!(gate.resume());
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn pause_gate_unblocks_on_cancellation() {
        let gate = PauseGate::default();
        let cancellation = CancellationToken::new();

        gate.pause();

        let waiter = tokio::spawn({
            let gate = gate.clone();
            let cancellation = cancellation.clone();
            async move { gate.checkpoint(&cancellation).await }
        });

        cancellation.cancel();
        assert!(!waiter.await.unwrap());
    }

    #[test]
    fn repeated_pause_and_resume_are_safe_no_ops() {
        let gate = PauseGate::default();

        assert!(gate.pause());
        assert!(!gate.pause());
        assert!(gate.resume());
        assert!(!gate.resume());
    }
}
```

- [ ] **Step 2: Run the pause-gate tests and verify failure**

Run:

```powershell
cargo test -p control-center-core pause_gate --locked
```

Expected: FAIL because `PauseGate` does not exist.

- [ ] **Step 3: Implement `PauseGate`**

Use an atomic flag plus `Notify`:

```rust
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct PauseGate {
    inner: Arc<PauseGateInner>,
}

#[derive(Default)]
struct PauseGateInner {
    paused: AtomicBool,
    resumed: Notify,
}

impl PauseGate {
    pub fn pause(&self) -> bool {
        !self.inner.paused.swap(true, Ordering::SeqCst)
    }

    pub fn resume(&self) -> bool {
        if !self.inner.paused.swap(false, Ordering::SeqCst) {
            return false;
        }
        self.inner.resumed.notify_waiters();
        true
    }

    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    pub async fn checkpoint(&self, cancellation: &CancellationToken) -> bool {
        loop {
            if cancellation.is_cancelled() {
                return false;
            }
            if !self.is_paused() {
                return true;
            }

            tokio::select! {
                _ = cancellation.cancelled() => return false,
                _ = self.inner.resumed.notified() => {}
            }
        }
    }
}
```

- [ ] **Step 4: Run the pause-gate tests**

Run:

```powershell
cargo test -p control-center-core pause_gate --locked
```

Expected: PASS.

- [ ] **Step 5: Add failing wire-contract tests for `ScanEvent`**

Define the expected public enums and serialize representative events:

```rust
#[test]
fn scan_event_wire_kinds_are_stable() {
    let cases = [
        (ScanEvent::Paused, "paused"),
        (ScanEvent::Resumed, "resumed"),
    ];

    for (event, expected) in cases {
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["kind"], expected);
    }
}

#[test]
fn progress_contains_optional_total_and_redacted_location_fields() {
    let value = serde_json::to_value(ScanEvent::Progress {
        scanner_id: "filesystem.deep".into(),
        completed_units: 12,
        total_units: None,
        current_location: Some("Selected root 1 · depth 3".into()),
    })
    .unwrap();

    assert_eq!(value["completed_units"], 12);
    assert!(value["total_units"].is_null());
    assert_eq!(value["current_location"], "Selected root 1 · depth 3");
}
```

Use this exact contract:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanScope {
    Quick,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanLifecycleState {
    Running,
    Paused,
    Cancelled,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanEvent {
    Started {
        scope: ScanScope,
        scanner_count: usize,
    },
    Progress {
        scanner_id: String,
        completed_units: u64,
        total_units: Option<u64>,
        current_location: Option<String>,
    },
    Discovery {
        discovery: Discovery,
    },
    ScannerFailed {
        scanner_id: String,
        code: String,
        message: String,
    },
    Paused,
    Resumed,
    Cancelled {
        visited: u64,
        discovered: u64,
        failure_count: u64,
        duration_ms: u64,
    },
    Completed {
        visited: u64,
        discovered: u64,
        failure_count: u64,
        duration_ms: u64,
    },
    Failed {
        code: String,
        message: String,
        failure_count: u64,
        duration_ms: u64,
    },
}
```

- [ ] **Step 6: Run the event-contract tests and verify failure**

Run:

```powershell
cargo test -p control-center-core scan_event --locked
```

Expected: FAIL because the existing event variants do not match the M6 contract.

- [ ] **Step 7: Add the bounded `ScanEventSink`**

Use channel capacity 128 at the caller. Only `Progress` may use `try_send`. All other event classes use awaited `send`.

```rust
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ScanEventSink {
    tx: mpsc::Sender<ScanEvent>,
    failures: Arc<AtomicU64>,
}

impl ScanEventSink {
    pub fn new(tx: mpsc::Sender<ScanEvent>) -> Self {
        Self {
            tx,
            failures: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn progress(&self, event: ScanEvent) {
        debug_assert!(matches!(event, ScanEvent::Progress { .. }));
        let _ = self.tx.try_send(event);
    }

    pub async fn critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        self.tx.send(event).await
    }

    pub async fn scanner_failed(
        &self,
        scanner_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        self.failures.fetch_add(1, Ordering::SeqCst);
        self.critical(ScanEvent::ScannerFailed {
            scanner_id: scanner_id.into(),
            code: code.into(),
            message: message.into(),
        })
        .await
    }

    pub fn failure_count(&self) -> u64 {
        self.failures.load(Ordering::SeqCst)
    }
}
```

Add one test that fills a capacity-1 channel with progress, proves a second progress call returns immediately, then proves a critical event is received after capacity becomes available.

- [ ] **Step 8: Migrate Quick orchestration to concurrency 4 and pause checkpoints**

In `scan.rs`:

```rust
pub const QUICK_SCAN_CONCURRENCY: usize = 4;
```

Change Quick orchestration to receive `ScanEventSink` and `PauseGate`. Before a queued scanner starts its external operation:

```rust
if !pause_gate.checkpoint(&cancellation).await {
    return ScannerTerminal::Cancelled;
}
```

Do not suspend a scanner that has already entered its bounded operation.

Replace old progress creation with:

```rust
events.progress(ScanEvent::Progress {
    scanner_id: scanner_id.to_owned(),
    completed_units: visited,
    total_units: None,
    current_location: None,
});
```

Send discoveries and scanner failures through the lossless sink methods. Track `Instant::now()` at coordinator start and include `events.failure_count()` plus elapsed milliseconds in terminal events. Check the root cancellation token before choosing the final terminal state so cancellation wins a race with child settlement.

Add a test with six blocked scanner jobs that records the maximum simultaneous runner count:

```rust
assert_eq!(QUICK_SCAN_CONCURRENCY, 4);
assert_eq!(max_seen.load(Ordering::SeqCst), 4);
```

Add a test that pauses before releasing queued jobs and asserts already-running jobs may finish but job 5 does not enter its runner until `resume()`.

- [ ] **Step 9: Export the shared M6 contracts**

In `lib.rs`:

```rust
pub mod scan_control;

pub use scan_control::{
    PauseGate, ScanEvent, ScanEventSink, ScanLifecycleState, ScanScope,
};
```

Move the old public `ScanEvent` export away from `scan.rs`.

- [ ] **Step 10: Run focused core tests**

Run:

```powershell
cargo test -p control-center-core scan --locked
```

Expected: PASS, including Quick concurrency 4, pause/cancel behavior, event wire shape and existing scanner isolation tests.

- [ ] **Step 11: Commit Task 1**

Run:

```powershell
git add engine/rust/control-center-core/src/scan_control.rs engine/rust/control-center-core/src/scan.rs engine/rust/control-center-core/src/lib.rs
git commit -m "feat: add shared scan lifecycle control"
```

---

### Task 2: Persist scan runs, scanner errors and scan-owned discoveries

**Files:**
- Modify: `engine/rust/control-center-core/src/storage.rs`
- Test: inline `#[cfg(test)]` module in `storage.rs`

**Interfaces:**
- Consumes: `ScanScope`, `ScanLifecycleState`, existing `Store`, existing `Discovery`.
- Produces:
  - `Store::begin_scan(scan_id, scope, started_at)`
  - `Store::set_scan_state(scan_id, state)`
  - `Store::finish_scan(scan_id, state, finished_at, failure_count)`
  - `Store::record_scan_error(scan_id, scanner_id, code, redacted_message, observed_at)`
  - `Store::enqueue_for_scan(scan_id, discovery)`
  - Nullable `discoveries.scan_id` for rows created by scan orchestration.
  - `scan_runs(id, scope, state, started_at, finished_at, failure_count)`
  - `scan_errors(id, scan_id, scanner_id, code, redacted_message, observed_at)`

- [ ] **Step 1: Add failing storage tests**

Create tests that use the existing temporary Store helper:

```rust
#[test]
fn scan_run_moves_from_running_to_completed() {
    let mut store = test_store();
    let scan_id = Uuid::new_v4();
    let started = Utc::now();

    store.begin_scan(scan_id, ScanScope::Quick, started).unwrap();
    store
        .finish_scan(
            scan_id,
            ScanLifecycleState::Completed,
            started + chrono::Duration::seconds(2),
            1,
        )
        .unwrap();

    let row = store.scan_run_for_test(scan_id).unwrap().unwrap();
    assert_eq!(row.scope, "quick");
    assert_eq!(row.state, "completed");
    assert_eq!(row.failure_count, 1);
    assert!(row.finished_at.is_some());
}

#[test]
fn scanner_error_is_owned_by_scan() {
    let mut store = test_store();
    let scan_id = Uuid::new_v4();
    let now = Utc::now();

    store.begin_scan(scan_id, ScanScope::Quick, now).unwrap();
    store
        .record_scan_error(
            scan_id,
            "python",
            "scanner_timeout",
            "scanner timed out",
            now,
        )
        .unwrap();

    let rows = store.scan_errors_for_test(scan_id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].scanner_id, "python");
    assert_eq!(rows[0].code, "scanner_timeout");
}

#[test]
fn discovery_created_by_scan_keeps_scan_id() {
    let mut store = test_store();
    let scan_id = Uuid::new_v4();
    let now = Utc::now();

    store.begin_scan(scan_id, ScanScope::Quick, now).unwrap();
    let discovery = sample_discovery();
    store.enqueue_for_scan(scan_id, &discovery).unwrap();

    assert_eq!(
        store.discovery_scan_id_for_test(discovery.id).unwrap(),
        Some(scan_id)
    );
}
```

- [ ] **Step 2: Run storage tests and verify failure**

Run:

```powershell
cargo test -p control-center-core storage --locked
```

Expected: FAIL because scan tables and methods do not exist.

- [ ] **Step 3: Add an idempotent schema upgrade**

Extend Store initialization with:

```sql
CREATE TABLE IF NOT EXISTS scan_runs (
    id TEXT PRIMARY KEY NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('quick', 'deep')),
    state TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    failure_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS scan_errors (
    id TEXT PRIMARY KEY NOT NULL,
    scan_id TEXT NOT NULL REFERENCES scan_runs(id) ON DELETE CASCADE,
    scanner_id TEXT NOT NULL,
    code TEXT NOT NULL,
    redacted_message TEXT NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_scan_errors_scan_id
    ON scan_errors(scan_id);
```

Before adding `discoveries.scan_id`, inspect `PRAGMA table_info(discoveries)`. If the column is absent, execute:

```sql
ALTER TABLE discoveries ADD COLUMN scan_id TEXT REFERENCES scan_runs(id);
```

Then create:

```sql
CREATE INDEX IF NOT EXISTS idx_discoveries_scan_id
    ON discoveries(scan_id);
```

Existing discoveries remain valid with `scan_id = NULL`.

- [ ] **Step 4: Implement scan persistence methods**

Use parameterized SQL only. Serialize scope and lifecycle state with the same snake-case strings as the event contract.

`begin_scan` inserts `running` and leaves `finished_at` null. `set_scan_state` updates only `state`. `finish_scan` accepts only `Cancelled`, `Completed` or `Failed`, then writes state, `finished_at` and `failure_count` in one statement. `record_scan_error` creates a new UUID for each row.

Keep existing `enqueue` behavior unchanged for non-scan callers. `enqueue_for_scan` performs the same fingerprint upsert and writes the owning scan id for the newly observed row without putting a raw path in `scan_runs`.

- [ ] **Step 5: Run storage tests**

Run:

```powershell
cargo test -p control-center-core storage --locked
```

Expected: PASS, including existing discovery and review tests.

- [ ] **Step 6: Commit Task 2**

Run:

```powershell
git add engine/rust/control-center-core/src/storage.rs
git commit -m "feat: persist scan lifecycle state"
```

---

### Task 3: Record forced Python process-tree termination without losing scan cancellation

**Files:**
- Modify: `engine/rust/control-center-core/src/python_supervisor.rs`
- Modify: `engine/rust/control-center-core/src/scan.rs`
- Test: existing Python supervisor and Quick Scan test modules

**Interfaces:**
- Consumes: existing Windows Job Object supervisor, root scan cancellation token, `ScanEventSink`.
- Produces:
  - `PythonTermination::Cooperative`
  - `PythonTermination::Forced`
  - Stable scanner error code `scanner_terminated` when the 2-second grace period expires.
  - Scan terminal state remains `cancelled` when root cancellation caused the forced process shutdown.

- [ ] **Step 1: Add a failing forced-termination test**

Extend the existing descendant-process cancellation fixture so the child ignores stdin shutdown long enough to exceed the 2-second grace period. Assert both outcomes:

```rust
assert_eq!(termination, PythonTermination::Forced);
assert!(!descendant_is_running(descendant_pid));
```

In the Quick adapter test, cancel the root token and assert the event stream contains:

```rust
ScanEvent::ScannerFailed {
    scanner_id,
    code,
    ..
} if scanner_id == "python" && code == "scanner_terminated"
```

and still ends with `ScanEvent::Cancelled { .. }`.

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```powershell
cargo test -p control-center-core python_supervisor --locked
cargo test -p control-center-core python --locked
```

Expected: FAIL because forced cancellation is currently collapsed into ordinary cancellation.

- [ ] **Step 3: Return a structured cancellation result from the supervisor**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonTermination {
    Cooperative,
    Forced,
}
```

When cancellation occurs:

1. Close child stdin.
2. Wait with `tokio::time::timeout(Duration::from_secs(2), child.wait())`.
3. If the child exits, return `Cooperative`.
4. If the grace period expires, close/drop the Job Object, await child settlement and return `Forced`.

Keep timeout behavior distinct from user cancellation.

- [ ] **Step 4: Map forced termination to a persisted scanner failure plus cancelled scan**

In the Python Quick adapter, when root cancellation is active and the supervisor reports `Forced`, call:

```rust
events
    .scanner_failed(
        "python",
        "scanner_terminated",
        "Python scanner process tree required forced termination",
    )
    .await?;
```

Then return `ScannerTerminal::Cancelled`.

Do not convert the whole scan to `Failed`. Cancellation remains the terminal lifecycle state, while `failure_count` includes the forced scanner termination through `ScanEventSink`.

- [ ] **Step 5: Run focused Python tests**

Run:

```powershell
cargo test -p control-center-core python_supervisor --locked
cargo test -p control-center-core python --locked
```

Expected: PASS, including existing Job Object descendant cleanup coverage.

- [ ] **Step 6: Commit Task 3**

Run:

```powershell
git add engine/rust/control-center-core/src/python_supervisor.rs engine/rust/control-center-core/src/scan.rs
git commit -m "fix: report forced scanner termination"
```

---

### Task 4: Windows Deep Scan safety policy

**Files:**
- Create: `engine/rust/control-center-core/src/deep_scan_windows.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `deep_scan_windows.rs`

**Interfaces:**
- Consumes: existing `windows` and `windows-sys` workspace dependencies.
- Produces:
  - `pub(crate) enum RootLocation { Local, Network }`
  - `pub(crate) struct EntryPolicy { pub reparse_point: bool, pub placeholder: bool }`
  - `pub(crate) struct DirectoryIdentity(String)`
  - `classify_root(path: &Path) -> io::Result<RootLocation>`
  - `entry_policy(path: &Path) -> io::Result<EntryPolicy>`
  - `stable_directory_identity(path: &Path) -> io::Result<DirectoryIdentity>`

- [ ] **Step 1: Add pure attribute-classification tests**

Use Windows constants rather than numeric literals in production:

```rust
#[test]
fn offline_and_recall_attributes_are_placeholders() {
    assert!(classify_attributes(FILE_ATTRIBUTE_OFFLINE).placeholder);
    assert!(classify_attributes(FILE_ATTRIBUTE_RECALL_ON_OPEN).placeholder);
    assert!(classify_attributes(FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS).placeholder);
}

#[test]
fn reparse_attribute_is_reported_separately() {
    let policy = classify_attributes(FILE_ATTRIBUTE_REPARSE_POINT);
    assert!(policy.reparse_point);
}
```

Add a path-shape test that `\\server\share\folder` is classified as network without touching the network.

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```powershell
cargo test -p control-center-core deep_scan_windows --locked
```

Expected: FAIL because the module does not exist.

- [ ] **Step 3: Implement root network classification**

On Windows:

- Treat a path beginning with a UNC prefix as `Network`.
- For drive-letter roots, call `GetDriveTypeW`.
- `DRIVE_REMOTE` is `Network`.
- Fixed, removable, RAM and CD-ROM drives are `Local`.
- Return an `io::Error` when Win32 cannot classify a syntactically valid selected root.

This catches mapped network drives as well as UNC roots.

For non-Windows compilation, keep a small `cfg(not(windows))` implementation that treats ordinary paths as local and paths beginning with `//` as network.

- [ ] **Step 4: Implement placeholder and reparse classification**

On Windows, use `std::os::windows::fs::MetadataExt::file_attributes()` and the official constants:

```rust
let placeholder =
    attributes & FILE_ATTRIBUTE_OFFLINE != 0
    || attributes & FILE_ATTRIBUTE_RECALL_ON_OPEN != 0
    || attributes & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0;

let reparse_point = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
```

This check happens before any file content operation. Deep Scan never opens placeholder file content in M6.

- [ ] **Step 5: Implement stable directory identity**

On Windows, open the directory with directory-compatible flags and call `GetFileInformationByHandle`. Build identity from `dwVolumeSerialNumber`, `nFileIndexHigh` and `nFileIndexLow`. Do not use `FILE_FLAG_OPEN_REPARSE_POINT` when following a reparse point because cycle prevention needs the target directory identity.

On non-Windows, use the canonical directory path as the fallback identity so tests and non-Windows builds remain deterministic.

- [ ] **Step 6: Add a stable-identity test**

For a temporary local directory:

```rust
let first = stable_directory_identity(temp.path()).unwrap();
let second = stable_directory_identity(temp.path()).unwrap();
assert_eq!(first, second);
```

- [ ] **Step 7: Run the policy tests**

Run:

```powershell
cargo test -p control-center-core deep_scan_windows --locked
```

Expected: PASS.

- [ ] **Step 8: Export the module internally**

In `lib.rs`:

```rust
mod deep_scan_windows;
```

Keep these low-level Win32 helpers crate-private.

- [ ] **Step 9: Commit Task 4**

Run:

```powershell
git add engine/rust/control-center-core/src/deep_scan_windows.rs engine/rust/control-center-core/src/lib.rs
git commit -m "feat: add deep scan filesystem policy"
```

---

### Task 5: Deep Scan traversal with explicit roots, concurrency 8 and cycle prevention

**Files:**
- Create: `engine/rust/control-center-core/src/deep_scan.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Modify: `engine/rust/control-center-core/src/scan.rs`
- Test: inline `#[cfg(test)]` module in `deep_scan.rs`

**Interfaces:**
- Consumes: `PauseGate`, `ScanEvent`, `ScanEventSink`, `ScanScope`, root classification and directory identity helpers, existing exclusion rules and discovery construction rules.
- Produces:
  - `pub const DEEP_DIRECTORY_CONCURRENCY: usize = 8`
  - `pub struct DeepScanContext { roots: Vec<PathBuf>, follow_reparse_points: bool, network_consent: bool }`
  - `pub enum DeepScanError`
  - `pub async fn deep_scan(context, events, cancellation, pause_gate)`
  - Scanner id `filesystem.deep`
  - Discovery source `filesystem.deep`

- [ ] **Step 1: Add failing root-safety tests**

Add tests for:

```rust
assert!(matches!(
    validate_roots(&[unc_root], false),
    Err(DeepScanError::NetworkConsentRequired)
));
assert!(validate_roots(&[unc_root], true).is_ok());
```

Add a test that an empty root list returns `DeepScanError::NoRootsSelected`.

- [ ] **Step 2: Add a fake filesystem for deterministic traversal tests**

Define a test-only implementation behind a small production trait:

```rust
trait DeepScanIo: Send + Sync + 'static {
    fn read_directory(&self, path: &Path) -> io::Result<Vec<DeepEntry>>;
    fn entry_policy(&self, path: &Path) -> io::Result<EntryPolicy>;
    fn directory_identity(&self, path: &Path) -> io::Result<DirectoryIdentity>;
}
```

`NativeDeepScanIo` delegates to the real filesystem and `deep_scan_windows` helpers.

The fake implementation stores a map of directory entries and atomic counters. Its `read_directory` sleeps for 25 ms while incrementing an active-read counter so concurrency can be measured without depending on disk timing.

- [ ] **Step 3: Add failing concurrency and pause tests**

Create at least 12 sibling directories under the fake root and assert:

```rust
assert_eq!(DEEP_DIRECTORY_CONCURRENCY, 8);
assert_eq!(fake.max_active_reads(), 8);
```

For pause:

1. Let the first batch enter `read_directory`.
2. Call `pause_gate.pause()`.
3. Release the in-flight batch.
4. Assert the next directory read has not started.
5. Call `resume()`.
6. Assert traversal finishes.

For cancellation while paused, pause before the next work item, cancel the token and assert terminal `Cancelled`.

- [ ] **Step 4: Add failing safety traversal tests**

Cover all of these cases with fake entries:

- A directory named `node_modules` is never read.
- A reparse directory is not queued when `follow_reparse_points` is false.
- A placeholder file is skipped and never becomes a discovery.
- With `follow_reparse_points` true, two paths that return the same `DirectoryIdentity` are read only once.
- A deliberately cyclic fake graph terminates.
- Progress `current_location` equals `Selected root 1 · depth 2` and contains no root path fragment.
- Discoveries use source `filesystem.deep`.
- Deep emits `Started { scope: Deep, scanner_count: 1 }`.

- [ ] **Step 5: Run Deep Scan tests and verify failure**

Run:

```powershell
cargo test -p control-center-core deep_scan --locked
```

Expected: FAIL because the Deep engine does not exist.

- [ ] **Step 6: Implement root validation**

Before traversal:

```rust
if context.roots.is_empty() {
    return Err(DeepScanError::NoRootsSelected);
}

for root in &context.roots {
    if classify_root(root)? == RootLocation::Network && !context.network_consent {
        return Err(DeepScanError::NetworkConsentRequired);
    }
}
```

Do not persist network consent after this request.

- [ ] **Step 7: Implement the bounded directory queue**

Use a `VecDeque<DirectoryWork>` and `JoinSet`. Each `DirectoryWork` stores:

```rust
struct DirectoryWork {
    path: PathBuf,
    root_index: usize,
    depth: usize,
}
```

Before launching each directory read:

```rust
if !pause_gate.checkpoint(&cancellation).await {
    emit_cancelled(...).await;
    return Ok(());
}
```

Never keep more than `DEEP_DIRECTORY_CONCURRENCY` directory-read tasks in the `JoinSet`.

Run blocking `read_dir` work through `tokio::task::spawn_blocking`. A directory read already in progress may settle after pause. Do not launch its children until the next pause checkpoint passes.

- [ ] **Step 8: Apply exclusions and metadata policy before queueing work**

Reuse the existing Quick exclusion-name list from `scan.rs` through a crate-private helper rather than maintaining two divergent lists.

For each entry:

1. Ignore excluded directory names.
2. Obtain `EntryPolicy`.
3. Skip placeholders.
4. If it is a reparse point and following is false, skip it.
5. If it is a directory and following is true, obtain target `DirectoryIdentity` and insert into a `HashSet`. Queue only when insertion is new.
6. If it is a relevant file, create a discovery with source `filesystem.deep`.

No file content read is required for M6 discovery.

- [ ] **Step 9: Emit privacy-safe progress and lossless discoveries**

After each directory work item settles:

```rust
events.progress(ScanEvent::Progress {
    scanner_id: "filesystem.deep".into(),
    completed_units: visited,
    total_units: None,
    current_location: Some(format!(
        "Selected root {} · depth {}",
        work.root_index + 1,
        work.depth
    )),
});
```

Send each discovery with `events.critical(ScanEvent::Discovery { discovery }).await`.

Recoverable `PermissionDenied`, disappearing-entry and per-directory metadata errors skip that work item and continue. Record one scanner failure event for each distinct stable error code, with a redacted message that does not contain the raw path. Do not emit an unbounded error for every inaccessible descendant.

- [ ] **Step 10: Emit terminal counts**

Track start `Instant`, visited directories, emitted discoveries and `events.failure_count()`. On root cancellation, emit `Cancelled`. On normal queue exhaustion, emit `Completed`. An unrecoverable coordinator invariant error emits `Failed`.

- [ ] **Step 11: Run Deep Scan tests**

Run:

```powershell
cargo test -p control-center-core deep_scan --locked
```

Expected: PASS, including concurrency 8, pause, cancel, placeholder skipping, no-follow default, cycle prevention and redacted location.

- [ ] **Step 12: Export Deep Scan**

In `lib.rs`:

```rust
pub mod deep_scan;

pub use deep_scan::{
    DEEP_DIRECTORY_CONCURRENCY, DeepScanContext, DeepScanError, deep_scan,
};
```

- [ ] **Step 13: Run all scan-focused core tests**

Run:

```powershell
cargo test -p control-center-core scan --locked
```

Expected: PASS for both Quick and Deep scan test names.

- [ ] **Step 14: Commit Task 5**

Run:

```powershell
git add engine/rust/control-center-core/src/deep_scan.rs engine/rust/control-center-core/src/scan.rs engine/rust/control-center-core/src/lib.rs
git commit -m "feat: add safe deep scan traversal"
```

---

### Task 6: Generic Tauri scan commands, revisions, persistence bridge and native picker

**Files:**
- Create: `apps/desktop/src-tauri/src/scan_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: inline `#[cfg(test)]` module in `scan_commands.rs`

**Interfaces:**
- Consumes: `quick_scan`, `deep_scan`, `Store`, `PauseGate`, `ScanEvent`, `ScanScope`, `ScanLifecycleState`, `CancellationToken`, `tauri_plugin_dialog::DialogExt`.
- Produces:
  - `bootstrap_state() -> BootstrapState` with `scan_revision`.
  - `pick_scan_roots() -> Vec<String>`.
  - `start_scan(ScanRequest) -> ScanHandle`.
  - `pause_scan(ScanMutationRequest) -> ScanState`.
  - `resume_scan(ScanMutationRequest) -> ScanState`.
  - `cancel_scan(ScanMutationRequest) -> ScanState`.
  - One `scan:event` channel.
  - Stable command errors `conflict`, `scan_active`, `scan_not_found`, `no_roots_selected`, `network_consent_required`, `storage_integrity` and `invalid_request`.

Use these request/response shapes:

```rust
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHandle {
    pub scan_id: String,
    pub scope: ScanScope,
    pub state: ScanLifecycleState,
    pub revision: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    pub scan_id: String,
    pub state: ScanLifecycleState,
    pub revision: String,
}
```

- [ ] **Step 1: Add Tauri dialog dependency**

In `apps/desktop/src-tauri/Cargo.toml`:

```toml
tauri-plugin-dialog = "2.7.2"
```

Run:

```powershell
cargo check -p ai-tool-control-center-desktop --locked
```

Expected: FAIL because the lockfile does not yet contain the new dependency.

Then run:

```powershell
cargo check -p ai-tool-control-center-desktop
```

Expected: PASS and `Cargo.lock` updates to include `tauri-plugin-dialog` 2.7.2.

- [ ] **Step 2: Add failing revision-state tests**

Extract state transitions into methods that can be tested without a Tauri window. Cover:

```rust
assert_eq!(
    registry.start_with_revision("stale", request).unwrap_err().code,
    "conflict"
);
```

For a current per-scan revision:

```rust
let paused = registry.pause(scan_id, current_revision).unwrap();
assert_eq!(paused.state, ScanLifecycleState::Paused);
assert_ne!(paused.revision, current_revision);

let no_op = registry.pause(scan_id, &paused.revision).unwrap();
assert_eq!(no_op.revision, paused.revision);
```

Then attempt resume with the pre-pause revision and assert `conflict`.

Add cancellation-while-paused coverage and assert the token becomes cancelled.

- [ ] **Step 3: Run desktop tests and verify failure**

Run:

```powershell
cargo test -p ai-tool-control-center-desktop scan_commands --locked
```

Expected: FAIL because the registry and generic commands do not exist.

- [ ] **Step 4: Implement `AppState` and revision rotation**

Move scan ownership out of `lib.rs`:

```rust
pub(crate) struct AppState {
    pub(crate) store: Mutex<Store>,
    scans: Mutex<HashMap<Uuid, ActiveScan>>,
    workspace_revision: Mutex<String>,
}

struct ActiveScan {
    cancellation: CancellationToken,
    pause_gate: PauseGate,
    scope: ScanScope,
    state: ScanLifecycleState,
    revision: String,
    started_at: DateTime<Utc>,
}
```

Generate opaque revisions with `Uuid::new_v4().to_string()`.

`bootstrap_state()` returns the current workspace revision. `start_scan` compares it before mutation and rotates it only after the scan has been accepted. Per-scan state transitions use the active scan revision.

Continue to allow only one active scan. A valid start while one is active returns `scan_active`.

- [ ] **Step 5: Implement native multi-folder selection**

Register:

```rust
.plugin(tauri_plugin_dialog::init())
```

Use Rust-side `DialogExt` and the non-blocking multi-folder callback. Bridge the callback through a Tokio oneshot channel:

```rust
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
```

Convert each desktop `FilePath` with `into_path()`, then to a String for display and the later `start_scan` request. Closing the picker returns an empty vector.

Do not add `dialog:*` capability permissions because JavaScript never invokes the plugin directly.

- [ ] **Step 6: Implement `start_scan`**

Order is mandatory:

1. Compare workspace revision.
2. Reject a second active scan.
3. Validate mode-specific request fields.
4. For Deep, reject zero roots.
5. Build `DeepScanContext` and let core root validation reject unconfirmed network roots.
6. Generate scan id, per-scan revision and root cancellation token.
7. Persist `scan_runs` in `running`.
8. Rotate workspace revision.
9. Insert `ActiveScan`.
10. Create `mpsc::channel(128)` and `ScanEventSink`.
11. Spawn Quick with scanner count 8 or Deep with scanner count 1.
12. Spawn the persistence/event bridge.
13. Return `ScanHandle`.

Remove `start_quick_scan` completely.

- [ ] **Step 7: Implement persistence-before-notification bridge**

For each core event:

- `Discovery`: call `store.enqueue_for_scan(scan_id, &discovery)`, then emit.
- `ScannerFailed`: call `store.record_scan_error(...)`, then emit.
- `Paused` and `Resumed`: these are emitted by their commands after `set_scan_state`, not duplicated by the runner.
- `Cancelled`, `Completed`, `Failed`: add any bridge-side storage failure count, call `finish_scan`, then emit terminal event and remove the active scan.

If discovery persistence fails, do not emit that discovery. Attempt to record `storage_integrity`; include it in failure count. If scan-run terminal persistence itself fails, emit a redacted `failed` event with code `storage_integrity` because there is no valid terminal record to announce as successful.

Every `window.emit("scan:event", event)` receives only `ScanEvent`; do not create additional scan event names.

- [ ] **Step 8: Implement pause and resume**

For `pause_scan`:

1. Parse id.
2. Compare per-scan revision.
3. If already paused, return current `ScanState`.
4. If running, call `pause_gate.pause()`.
5. Rotate per-scan revision.
6. Update in-memory state to `Paused`.
7. Persist `scan_runs.state = paused`.
8. Emit `ScanEvent::Paused`.
9. Return the new `ScanState`.

`resume_scan` mirrors the sequence with `resume()`, state `Running` and `ScanEvent::Resumed`.

- [ ] **Step 9: Implement cancel**

For `cancel_scan`:

1. Parse id.
2. Compare per-scan revision.
3. If cancellation was already requested with the current revision, return current state.
4. Set active in-memory state to `Cancelled`.
5. Rotate per-scan revision.
6. Call the root `CancellationToken::cancel()`.
7. Return `ScanState { state: Cancelled, ... }`.

Do not persist the terminal scan-run state or emit `cancelled` from the command. The runner bridge does both after bounded work and owned processes settle.

- [ ] **Step 10: Cancel active scans on main-window close**

In `lib.rs`, register a window event handler. On `WindowEvent::CloseRequested`, get `AppState` and call a synchronous `cancel_all()` that only cancels tokens and opens pause gates. Do not block the window event thread waiting for scans.

The Python Job Object remains kill-on-close, so dropping the owned supervisor closes descendants even if runtime shutdown follows immediately.

- [ ] **Step 11: Register the generic commands**

The invoke handler contains:

```rust
tauri::generate_handler![
    bootstrap_state,
    review_discovery,
    pick_scan_roots,
    start_scan,
    pause_scan,
    resume_scan,
    cancel_scan,
]
```

There is no `start_quick_scan`.

- [ ] **Step 12: Run desktop tests**

Run:

```powershell
cargo test -p ai-tool-control-center-desktop --locked
```

Expected: PASS for revision conflicts, safe no-ops, cancellation while paused, mode validation and existing runtime-root tests.

- [ ] **Step 13: Verify the old command is gone**

Run:

```powershell
rg "start_quick_scan|startQuickScan|cancelQuickScan" apps engine
```

Expected: no matches after frontend migration is complete. At this Task 6 checkpoint, frontend matches are allowed, but Rust/Tauri matches must be absent.

- [ ] **Step 14: Commit Task 6**

Run:

```powershell
git add Cargo.lock apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/scan_commands.rs
git commit -m "feat: expose generic scan commands"
```

---

### Task 7: Generic frontend scan API and exact event types

**Files:**
- Modify: `apps/frontend/src/model.ts`
- Modify: `apps/frontend/src/api.ts`
- Modify: `apps/frontend/src/App.desktop.test.tsx`

**Interfaces:**
- Consumes: Tauri commands from Task 6 and the `scan:event` wire contract.
- Produces:
  - `ScanMode = "quick" | "deep"`
  - `ScanRequest`
  - `ScanHandle`
  - `ScanState`
  - `ScanEvent`
  - `pickScanRoots()`
  - `startScan(request, onEvent)`
  - `pauseScan(request)`
  - `resumeScan(request)`
  - `cancelScan(request)`
  - `BootstrapState.scanRevision`

- [ ] **Step 1: Replace frontend scan model types**

Use exact camel-case JSON property names from Tauri:

```ts
export type ScanMode = "quick" | "deep";
export type ScanLifecycleState =
  | "running"
  | "paused"
  | "cancelled"
  | "completed"
  | "failed";

export interface ScanRequest {
  mode: ScanMode;
  roots: string[];
  followReparsePoints: boolean;
  networkConsent: boolean;
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
```

Define the discriminated `ScanEvent` union with all nine `kind` values and the terminal fields from Task 1.

- [ ] **Step 2: Update desktop tests to expect generic calls**

Mock:

```ts
vi.mock("./api", () => ({
  bootstrap: vi.fn(),
  pickScanRoots: vi.fn(),
  startScan: vi.fn(),
  pauseScan: vi.fn(),
  resumeScan: vi.fn(),
  cancelScan: vi.fn(),
  reviewDiscovery: vi.fn(),
  isDesktop: vi.fn(() => true),
}));
```

Add a failing test that starts Quick and expects:

```ts
expect(startScan).toHaveBeenCalledWith(
  expect.objectContaining({
    mode: "quick",
    roots: [],
    followReparsePoints: false,
    networkConsent: false,
    revision: "workspace-r1",
  }),
  expect.any(Function),
);
```

- [ ] **Step 3: Run frontend tests and verify failure**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test --run
```

Expected: FAIL because `startScan`, mutation wrappers and the new model types are not implemented.

- [ ] **Step 4: Implement the generic Tauri wrappers**

`startScan` must attach the event listener before invoking the command so an immediate `started` event cannot be missed:

```ts
export async function startScan(
  request: ScanRequest,
  onEvent: (event: ScanEvent) => void,
): Promise<{ handle: ScanHandle; unlisten: UnlistenFn }> {
  const unlisten = await listen<ScanEvent>("scan:event", ({ payload }) => {
    onEvent(payload);
  });

  try {
    const handle = await invoke<ScanHandle>("start_scan", { request });
    return { handle, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}
```

Mutation wrappers pass one `request` envelope:

```ts
export const pauseScan = (request: ScanMutationRequest) =>
  invoke<ScanState>("pause_scan", { request });

export const resumeScan = (request: ScanMutationRequest) =>
  invoke<ScanState>("resume_scan", { request });

export const cancelScan = (request: ScanMutationRequest) =>
  invoke<ScanState>("cancel_scan", { request });

export const pickScanRoots = () =>
  invoke<string[]>("pick_scan_roots");
```

Extend `bootstrap()` response typing with `scanRevision: string`.

Remove `startQuickScan` and `cancelQuickScan`.

- [ ] **Step 5: Run frontend tests**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test --run
```

Expected: tests may still fail on App UI assumptions, but there are no TypeScript/API failures for removed scan wrappers. Task 8 resolves the UI assertions.

- [ ] **Step 6: Commit Task 7**

Run:

```powershell
git add apps/frontend/src/model.ts apps/frontend/src/api.ts apps/frontend/src/App.desktop.test.tsx
git commit -m "feat: add generic scan frontend api"
```

---

### Task 8: Run Scan dialog, Deep options and persistent lifecycle controls

**Files:**
- Modify: `apps/frontend/src/App.tsx`
- Modify: `apps/frontend/src/styles.css`
- Modify: `apps/frontend/src/App.desktop.test.tsx`
- Modify: `apps/frontend/src/App.test.tsx`

**Interfaces:**
- Consumes: generic API and model types from Task 7.
- Produces:
  - One `Run scan` button.
  - Compact Quick/Deep dialog.
  - Native-root picker for Deep.
  - Per-scan `Follow symbolic links and junctions` option, default off.
  - Per-scan network confirmation retry.
  - Persistent scan bar with progress, redacted current location, pause/resume and cancel.
  - Terminal refresh of `bootstrap_state()` to obtain the next workspace revision.

- [ ] **Step 1: Add failing dialog tests**

For desktop mode, assert:

1. The main action is `Run scan`, not `Run quick scan`.
2. Clicking it opens a dialog with Quick selected.
3. Choosing Deep reveals `Select folders`.
4. `Follow symbolic links and junctions` is unchecked.
5. `Run` is disabled for Deep until at least one root is selected.
6. Selected roots are shown only inside the dialog.

Example:

```ts
await user.click(screen.getByRole("button", { name: "Run scan" }));
await user.click(screen.getByRole("radio", { name: "Deep" }));

expect(
  screen.getByRole("checkbox", {
    name: "Follow symbolic links and junctions",
  }),
).not.toBeChecked();

expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
```

- [ ] **Step 2: Add failing lifecycle-control tests**

Start a scan with handle revision `scan-r1`. Then:

- `paused` event changes the scan bar label to Paused.
- Clicking Resume calls `resumeScan({ scanId, revision: currentRevision })`.
- A successful resume response replaces the stored revision.
- Cancel uses the newest revision.
- A `scanner_failed` event shows a warning but does not hide prior discoveries.
- `completed`, `cancelled` or `failed` removes active controls but leaves a terminal notice.
- After terminal, `bootstrap()` is called again and the returned `scanRevision` replaces the previous workspace revision.

- [ ] **Step 3: Add a failing network-consent retry test**

Mock the first Deep `startScan` call to reject with `{ code: "network_consent_required" }`. Assert a confirmation UI explicitly names network scanning without storing consent globally.

After the user confirms, assert the second request is identical except:

```ts
networkConsent: true
```

Closing the dialog and opening a new Deep Scan starts again with `networkConsent: false`.

- [ ] **Step 4: Run frontend tests and verify failure**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test --run
```

Expected: FAIL on the new dialog and lifecycle assertions.

- [ ] **Step 5: Implement the dialog state**

Use one local draft:

```ts
const [scanDraft, setScanDraft] = useState({
  open: false,
  mode: "quick" as ScanMode,
  roots: [] as string[],
  followReparsePoints: false,
  networkConsent: false,
});
```

Opening the dialog resets all four values, which guarantees no network or reparse consent leaks into a later scan.

`Select folders` calls `pickScanRoots()` and replaces the draft root list with the returned selection.

- [ ] **Step 6: Start both modes through one function**

Construct:

```ts
const request: ScanRequest = {
  mode: scanDraft.mode,
  roots: scanDraft.mode === "deep" ? scanDraft.roots : [],
  followReparsePoints:
    scanDraft.mode === "deep" && scanDraft.followReparsePoints,
  networkConsent:
    scanDraft.mode === "deep" && scanDraft.networkConsent,
  revision: workspaceRevision,
};
```

Quick never sends selected roots or reparse/network options as enabled.

When `startScan` resolves, store `handle.scanId`, `handle.revision`, state and scope. Keep the event unlisten function until terminal or component unmount.

- [ ] **Step 7: Implement network confirmation**

If start fails with `network_consent_required`, keep the dialog open and show a compact confirmation panel:

`One or more selected roots are on a network location. Allow this Deep Scan to read those network roots once?`

Confirming sets draft `networkConsent` to true and retries. Cancelling leaves it false.

Do not write this value to localStorage, preferences or bootstrap state.

- [ ] **Step 8: Implement the persistent scan bar**

The scan bar remains mounted while navigating between app sections. Show:

- scope label `Quick scan` or `Deep scan`
- running/paused/cancelling text
- scanner id when present
- completed units
- total only when non-null
- `currentLocation` exactly as provided by backend
- Pause when running
- Resume when paused
- Cancel while running or paused

Do not derive or display a raw path.

Mutation responses replace the stored per-scan revision immediately.

- [ ] **Step 9: Preserve discoveries and warnings during scanner failures**

`scanner_failed` appends a warning identified by scanner id and stable code. It does not clear the discovery list and does not make the whole UI modal.

`discovery` uses the same review/inventory data flow as before.

- [ ] **Step 10: Refresh workspace revision after terminal**

On `cancelled`, `completed` or `failed`:

1. Store terminal notice and counts.
2. Clear active controls.
3. Call the active event unlisten function.
4. Call `bootstrap()`.
5. Replace `workspaceRevision` with the returned `scanRevision`.
6. Refresh pending discoveries and inventory from the same bootstrap result.

This makes `Run scan` immediately usable again with the rotated workspace revision.

- [ ] **Step 11: Keep browser demo mode behaviorally equivalent**

The non-Tauri demo runner uses the same React state transitions as desktop:

- emits a slow progress sequence
- allows pause/resume
- allows cancellation while paused
- produces at least one recoverable scanner warning while continuing
- reaches a terminal state
- allows another scan to start

The browser demo does not read the filesystem.

- [ ] **Step 12: Style the compact modal and scan bar**

In `styles.css`, keep the existing visual language. The dialog must fit without full-screen takeover at the existing 1280x820 desktop window. Selected roots scroll inside the dialog rather than expanding the page.

The persistent scan bar must not cover navigation or review controls.

- [ ] **Step 13: Run frontend tests**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test --run
```

Expected: PASS.

- [ ] **Step 14: Run TypeScript build**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend build
```

Expected: PASS.

- [ ] **Step 15: Commit Task 8**

Run:

```powershell
git add apps/frontend/src/App.tsx apps/frontend/src/styles.css apps/frontend/src/App.desktop.test.tsx apps/frontend/src/App.test.tsx
git commit -m "feat: add scan mode dialog and controls"
```

---

### Task 9: Playwright acceptance coverage for responsiveness and restart

**Files:**
- Modify: `apps/frontend/package.json`
- Modify: `pnpm-lock.yaml`
- Create: `apps/frontend/playwright.config.ts`
- Create: `apps/frontend/e2e/scan-orchestration.spec.ts`

**Interfaces:**
- Consumes: browser demo scan behavior from Task 8.
- Produces:
  - `pnpm --filter @ai-tool-control-center/frontend test:e2e`
  - Browser acceptance proof that navigation and review stay interactive during a deliberately slow scan.
  - Pause/resume/cancel acceptance.
  - Recoverable warning isolation.
  - Ability to start a second scan after terminal state.

- [ ] **Step 1: Add Playwright dependency and script**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend add -D @playwright/test@1.62.1
```

Add:

```json
"test:e2e": "playwright test"
```

to frontend scripts.

- [ ] **Step 2: Create Playwright configuration**

Use:

```ts
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  use: {
    baseURL: "http://127.0.0.1:1420",
  },
  webServer: {
    command: "pnpm dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
```

- [ ] **Step 3: Write the slow-scan responsiveness test**

The test must prove interaction during active scanning, not only before and after:

```ts
import { expect, test } from "@playwright/test";

test("navigation and review remain interactive during a slow scan", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Run scan" }).click();
  await page.getByRole("button", { name: "Run" }).click();

  await expect(page.getByText(/Quick scan/i)).toBeVisible();
  await expect(page.getByText(/Running/i)).toBeVisible();

  await page.getByRole("button", { name: /Pending/i }).click();
  await expect(page.getByRole("main")).toBeVisible();

  const firstReview = page.getByRole("button", { name: /Review/i }).first();
  await firstReview.click();
  await expect(page.getByText(/Running/i)).toBeVisible();

  await expect(page.getByText(/scanner/i)).toBeVisible();
});
```

Use the app's actual navigation accessible names if they differ. The assertion must show the scan is still active after navigation/review interaction.

- [ ] **Step 4: Write pause, resume and cancel test**

Sequence:

1. Start Quick.
2. Wait for completed units to advance.
3. Click Pause.
4. Record displayed completed units.
5. Wait at least one demo progress interval and assert the value does not advance after any already-scheduled bounded step settles.
6. Click Resume.
7. Assert units advance.
8. Click Cancel.
9. Assert terminal cancellation notice.
10. Open `Run scan` again and start another scan successfully.

- [ ] **Step 5: Write recoverable failure isolation test**

During the demo's deliberately failing scanner:

```ts
await expect(page.getByText(/scanner.*failed/i)).toBeVisible();
await expect(page.getByText(/Running/i)).toBeVisible();
```

Then wait for another progress/discovery update and prove the scan did not terminate because one scanner warning occurred.

- [ ] **Step 6: Install Chromium if this machine does not already have the Playwright browser**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend exec playwright install chromium
```

Expected: command exits 0.

- [ ] **Step 7: Run E2E**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test:e2e
```

Expected: PASS for responsiveness, pause/resume/cancel, warning isolation and restart.

- [ ] **Step 8: Commit Task 9**

Run:

```powershell
git add apps/frontend/package.json pnpm-lock.yaml apps/frontend/playwright.config.ts apps/frontend/e2e/scan-orchestration.spec.ts
git commit -m "test: cover scan orchestration e2e"
```

---

### Task 10: Milestone 6 reconciliation and full verification

**Files:**
- Modify only files that fail the checks below. Do not broaden scope.
- Verify against:
  - `docs/superpowers/specs/2026-08-15-m6-scan-orchestration-completion-design.md`
  - `docs/superpowers/plans/2026-07-23-ai-tool-control-center-open-source-windows-implementation.md`
  - `docs/superpowers/specs/2026-07-21-ai-tool-control-center-open-source-windows-design.md`

**Interfaces:**
- Consumes: all Tasks 1 through 9.
- Produces: a clean Milestone 6 branch with all required test gates passing and no Quick-only public API left.

- [ ] **Step 1: Run the master M6 Rust gate**

Run:

```powershell
cargo test -p control-center-core scan --locked
```

Expected: PASS.

- [ ] **Step 2: Run all Rust workspace tests**

Run:

```powershell
cargo test --workspace --locked
```

Expected: PASS.

- [ ] **Step 3: Run the master frontend unit gate**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test --run
```

Expected: PASS.

- [ ] **Step 4: Run the master E2E gate**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend test:e2e
```

Expected: PASS.

- [ ] **Step 5: Run frontend build and lint**

Run:

```powershell
pnpm --filter @ai-tool-control-center/frontend build
pnpm --filter @ai-tool-control-center/frontend lint
```

Expected: both PASS.

- [ ] **Step 6: Prove the clean API migration**

Run:

```powershell
rg "start_quick_scan|startQuickScan|cancelQuickScan" apps engine
```

Expected: no matches.

Run:

```powershell
rg 'start_scan|pause_scan|resume_scan|cancel_scan|pick_scan_roots' apps/desktop/src-tauri apps/frontend/src
```

Expected: all five new commands appear in their intended Rust/API locations.

- [ ] **Step 7: Prove concurrency constants and scanner identities**

Run:

```powershell
rg "QUICK_SCAN_CONCURRENCY|DEEP_DIRECTORY_CONCURRENCY|filesystem\.deep" engine/rust/control-center-core/src
```

Expected:
- Quick constant is 4.
- Deep directory constant is 8.
- Deep scanner id/source is `filesystem.deep`.

- [ ] **Step 8: Prove the event channel is singular**

Run:

```powershell
rg 'scan:event|scan\.(started|progress|discovery|scanner_failed|paused|resumed|cancelled|completed|failed)' apps engine
```

Expected: runtime emission/listening uses `scan:event`; there are no separate Tauri channels named after conceptual scan events.

- [ ] **Step 9: Prove raw paths are not persisted as scope**

Inspect the scan persistence calls and assert `begin_scan` receives only `ScanScope`. Run:

```powershell
rg "begin_scan|scan_runs|current_location|Selected root" engine/rust/control-center-core/src apps/desktop/src-tauri/src
```

Expected:
- `scan_runs.scope` receives only `quick` or `deep`.
- progress location is constructed as `Selected root N · depth D`.
- no selected root string is passed into `scan_runs.scope` or an event location.

- [ ] **Step 10: Prove network and reparse defaults**

Run:

```powershell
rg "network_consent|follow_reparse_points|NetworkConsentRequired|FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_OFFLINE|FILE_ATTRIBUTE_RECALL" apps engine
```

Expected:
- frontend draft defaults both consent flags to false.
- core rejects network roots without per-request consent.
- reparse following is conditional.
- cloud placeholder attributes are explicitly skipped.

- [ ] **Step 11: Prove persistence-before-notification ordering by tests**

Add or retain focused bridge tests whose call log asserts these exact sequences:

```text
persist_discovery
emit_discovery
```

```text
persist_scan_error
emit_scanner_failed
```

```text
persist_terminal
emit_terminal
```

Run:

```powershell
cargo test -p ai-tool-control-center-desktop persistence_before_notification --locked
```

Expected: PASS.

- [ ] **Step 12: Prove stale revision conflicts**

Run:

```powershell
cargo test -p ai-tool-control-center-desktop revision --locked
```

Expected: PASS for stale workspace start, stale pause/resume/cancel and safe current-revision no-ops.

- [ ] **Step 13: Check formatting**

Run:

```powershell
cargo fmt --all -- --check
```

Expected: PASS. If it reports formatting changes, run `cargo fmt --all`, inspect the diff and rerun the check.

- [ ] **Step 14: Inspect the final diff**

Run:

```powershell
git status --short
git diff --check
git log --oneline --decorate -12
```

Expected:
- `git diff --check` prints nothing.
- no generated schema file was manually edited.
- no unrelated file is modified.
- Task commits are present after the committed M6 design spec.

- [ ] **Step 15: Commit only reconciliation changes if Step 1 through Step 14 required code changes**

If reconciliation changed tracked files, stage only those files and commit:

```powershell
git commit -m "feat: add cancellable scan orchestration"
```

If reconciliation made no changes, do not create an empty commit.

## Plan Self-Review Record

**Spec coverage:** Tasks 1 through 10 cover Quick concurrency 4, cooperative pause, cancellation while paused, bounded event backpressure, slow/failing scanner isolation, Deep explicit-root traversal, directory concurrency 8, reparse defaults, cycle prevention, cloud-placeholder skipping, per-scan network consent, native root selection, generic commands, revisions, persistence ordering, redacted progress, window-close cancellation, React controls and Playwright responsiveness.

**Placeholder scan:** This plan contains no deferred implementation markers. Every code-changing task names concrete interfaces, tests, commands and expected outcomes.

**Type consistency:** `ScanScope`, `ScanLifecycleState`, `PauseGate`, `ScanEvent`, `ScanEventSink`, `ScanRequest`, `ScanMutationRequest`, `ScanHandle` and `ScanState` retain the same names and field semantics from their defining task through desktop and frontend tasks.

**Master reconciliation:** Quick reports scanner count 8 and uses concurrency 4. Deep reports scanner count 1 and uses directory-read concurrency 8. Only progress is lossy. Discovery, scanner failure and terminal notification are persistence-ordered. Cancellation retains precedence. Forced Python shutdown records `scanner_terminated`. Public Quick-only start/cancel wrappers are removed.
