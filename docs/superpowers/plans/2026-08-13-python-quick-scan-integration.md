# Python Quick Scan Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `python.config` as the eighth stable Windows Quick Scan scanner, using only bundled CPython and the existing coordinator, persistence and Review Queue path.

**Architecture:** The Rust core receives a `QuickScanContext` containing discovery roots plus a fallible Python application root. A focused Python job adapter runs the existing `run_python_scan` supervisor inside a normal `ScannerJob`, forwards discoveries as ordinary `ScanEvent::Discovery` events and isolates Python failures. The Tauri shell owns development-versus-packaged application-root resolution and carries resolution failure into the core instead of rejecting Quick Scan.

**Tech Stack:** Rust 1.96.0, Tokio, tokio-util `CancellationToken`, Tauri 2.11.5, bundled CPython 3.14.7, PowerShell, pnpm 10.33.2.

## Global Constraints

- Stable Windows scanner order: `filesystem.quick`, `windows.known_location`, `windows.path`, `windows.uninstall_registry`, `windows.process`, `windows.service`, `windows.tcp`, `python.config`.
- `python.config` is a normal `ScannerJob` using concurrency `3`, timeout `60 seconds`, child cancellation, progress, panic isolation and normal settlement.
- The only interpreter path is `<app_root>\runtimes\cpython-3.14.7-windows-x86_64\python.exe`; never fall back to `PATH`, `python`, `python3`, `py`, `PYTHONHOME` or `PYTHONPATH`.
- Development root comes from compile-time `CARGO_MANIFEST_DIR`; packaged root comes from the running executable parent. Both must be absolute.
- Python root or runtime failure fails only `python.config`; the seven native scanners continue and global terminal state waits for all eight jobs.
- Preserve `scanner_protocol`, `scanner_timeout`, `scanner_cancelled` and `scanner_failed`.
- Python discoveries use the existing Tauri persistence-before-emission path and enter the pending Review Queue.
- Rust unit tests must not require a staged `runtimes/` directory because CI runs Rust tests before runtime staging.
- Do not duplicate existing supervisor protocol-bound, stderr-redaction, timeout-internal, descendant-cleanup or Job Object tests.
- No frontend redesign, final installer packaging or `0.2` version bump in this slice.

---

### Task 1: Add `python.config` to the core Quick Scan coordinator

**Files:**
- Modify: `engine/rust/control-center-core/src/scan.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline tests in `engine/rust/control-center-core/src/scan.rs`

**Interfaces:**
- Consumes: `run_python_scan(&Path, &[PathBuf], Duration, CancellationToken, FnMut(Discovery)) -> Result<u64, PythonSupervisorError>`.
- Produces: `pub struct PythonRootError;`.
- Produces: `pub struct QuickScanContext { pub roots: Vec<PathBuf>, pub python_app_root: Result<PathBuf, PythonRootError> }`.
- Produces: `pub async fn quick_scan(context: QuickScanContext, events: mpsc::Sender<ScanEvent>, cancellation: CancellationToken)`.
- Produces internally: `build_quick_scan_jobs(context: QuickScanContext) -> Vec<ScannerJob>` and `build_python_config_job_with_runner(...) -> ScannerJob`.

- [ ] **Step 1: Write failing stable-ID and job-list tests.** Update the existing expected scanner slice to append `python.config`, and build jobs with a context that does not require a real runtime:

```rust
let jobs = build_quick_scan_jobs(QuickScanContext {
    roots: Vec::new(),
    python_app_root: Err(PythonRootError),
});
let scanner_ids: Vec<&str> = jobs.iter().map(|job| job.scanner_id.as_str()).collect();
assert_eq!(scanner_ids, quick_scan_scanner_ids());
```

- [ ] **Step 2: Run the two focused tests and confirm RED.**

```powershell
cargo test -p control-center-core quick_scan_exposes_stable_scanner_ids --locked -j 1 -- --nocapture
cargo test -p control-center-core quick_scan_builds_every_stable_scanner_job --locked -j 1 -- --nocapture
```

Expected: failure because the eighth ID and context types do not exist yet.

- [ ] **Step 3: Add the context types and public export.** In `scan.rs` add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PythonRootError;

#[derive(Debug, Clone)]
pub struct QuickScanContext {
    pub roots: Vec<PathBuf>,
    pub python_app_root: Result<PathBuf, PythonRootError>,
}
```

Re-export from `lib.rs`:

```rust
pub use scan::{PythonRootError, QuickScanContext, ScanEvent, quick_scan};
```

Change `quick_scan` to consume `QuickScanContext` and use `context.roots` on non-Windows builds.

- [ ] **Step 4: Add the Python job adapter with an injectable runner.** Add a Windows-only helper whose runner receives already translated `Discovery` values through an unbounded bridge:

```rust
#[cfg(indows)]
fn build_python_config_job_with_runner<F, Fut>(
    app_root: Result<PathBuf, PythonRootError>,
    roots: Vec<PathBuf>,
    runner: F,
) -> ScannerJob
where
    F: FnOnce(
            PathBuf,
            Vec<PathBuf>,
            Duration,
            CancellationToken,
            mpsc::UnboundedSender<Discovery>,
        ) -> Fut
        + Send
        + 'static,
    Fut: Future<Output = Result<u64, crate::python_supervisor::PythonSupervisorError>>
        + Send
        + 'static,
{
    ScannerJob::new(
        "python.config",
        QUICK_SCAN_SCANNER_TIMEOUT,
        move |events, cancellation| async move {
            let app_root = match app_root {
                Ok(app_root) => app_root,
                Err(_) => {
                    return ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "Python application root is unavailable".into(),
                    };
                }
            };

            let (discovery_tx, mut discovery_rx) = mpsc::unbounded_channel();
            let scan = runner(
                app_root,
                roots,
                QUICK_SCAN_SCANNER_TIMEOUT,
                cancellation,
                discovery_tx,
            );
            tokio::pin!(scan);

            let mut forwarded = 0_u64;
            let mut discovery_channel_open = true;

            loop {
                tokio::select! {
                    result = &mut scan => {
                        while let Ok(discovery) = discovery_rx.try_recv() {
                            if events.send(ScanEvent::Discovery { discovery }).await.is_err() {
                                return ScannerTerminal::Failed {
                                    code: "event_channel_closed".into(),
                                    message: "Scan event channel closed while publishing discoveries".into(),
                                };
                            }
                            forwarded = forwarded.saturating_add(1);
                        }

                        return match result {
                            Ok(count) => ScannerTerminal::Completed {
                                visited: 0,
                                discovered: count,
                            },
                            Err(error) if error.code() == "scanner_cancelled" => {
                                ScannerTerminal::Cancelled {
                                    visited: 0,
                                    discovered: forwarded,
                                }
                            }
                            Err(error) => ScannerTerminal::Failed {
                                code: error.code().into(),
                              message: "Python scanner failed".into(),
                            },
                        };
                    }
                    discovery = discovery_rx.recv(), if discovery_channel_open => {
                        match discovery {
                            Some(discovery) => {
                                if events.send(ScanEvent::Discovery { discovery }).await.is_err() {
                                return ScannerTerminal::Failed {
                                        code: "event_channel_closed".into(),
                                      message: "Scan event channel closed while publishing discoveries".into(),
                                    };
                                }
                                forwarded = forwarded.saturating_add(1);
                            }
                            None => discovery_channel_open = false,
                        }
                    }
                }
            }
        },
    )
}
```

The production wrapper must call the existing supervisor and only adapt its synchronous discovery callback into the bridge:

```rust
#[cfg(windows)]
fn build_python_config_job(
    app_root: Result<PathBuf, PythonRootError>,
    roots: Vec<PathBuf>,
) -> ScannerJob {
    build_python_config_job_with_runner(
        app_root,
        roots,
        |app_root, roots, timeout, cancellation, discoveries| async move {
            crate::python_supervisor::run_python_scan(
                &app_root,
                &roots,
                timeout,
                cancellation,
                move |discovery| {
                    let _ = discoveries.send(discovery);
                },
            )
            .await
        },
    )
}
```

- [ ] **Step 5: Append the production Python job to the Windows Quick Scan list.** Change `build_quick_scan_jobs` to accept `QuickScanContext`, split out a cloned Python root list before the filesystem closure consumes the original list, and append exactly one `build_python_config_job(...)` after `windows.tcp`.

```rust
#[cfg(indows)]
fn build_quick_scan_jobs(context: QuickScanContext) -> Vec<ScannerJob> {
    let python_roots = context.roots.clone();
    let roots = context.roots;
    let python_app_root = context.python_app_root;

    // existing seven jobs, unchanged in order
    let mut jobs = vec[![
        // filesystem.quick through windows.tcp
    ];

    jobs.push(build_python_config_job(python_app_root, python_roots));

    debug_assert_eq!(
        jobs.iter()
            .map(|job| job.scanner_id.as_str())
            .collect::<Vec<_>>(),
        quick_scan_scanner_ids()
    );

    jobs
}
```

Do not rewrite the seven existing native job bodies. The `// existing seven jobs` comment above describes the edit location only; the implementation must retain their current code verbatim and append the Python job.

- [ ] **Step 6: Write the failing Python forwarding/count test before completing the adapter.** The test must use the injected runner, not the staged runtime:

```rust
#[cfg(indows)]
#[tokio::test]
async fn python_config_job_forwards_discoveries_and_counts_them() {
    let app_root = tempdir().unwrap();
    let expected_root = app_root.path().to_path_buf();
    let expected_root_for_runner = expected_root.clone();

    let mut discovery = Discovery::unknown("Python config", "python.config", "python-config-test");
    discovery.evidence.push(Evidence {
        kind: "config".into(),
        summary: "test config".into(),
    });

    let job = build_python_config_job_with_runner(
        Ok(expected_root),
        vec![PathBuf::from(r"C:\\scan-root")],
        move |app_root, roots, _timeout, _cancellation, discoveries| async move {
            assert_eq!(app_root, expected_root_for_runner);
            assert_eq!(roots, vec![PathBuf::from(r"C:\\scan-root")]);
            discoveries.send(discovery).unwrap();
            Ok::<u64, crate::python_supervisor::PythonSupervisorError>(1)
        },
    );

    let (sender, mut receiver) = mpsc::channel(16);
    run_scanner_jobs(vec![job], 1, sender, CancellationToken::new()).await;

    let mut saw_discovery = false;
    let mut saw_completed = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            ScanEvent::Discovery { discovery }
                if discovery.source_scanner == "python.config"
                    && discovery.suggested_name == "Python config" =>
            {
                saw_discovery = true;
            }
            ScanEvent::Completed {
                discovered: 1, ..
            } => saw_completed = true,
            _ => {}
        }
    }

    assert!(saw_discovery);
    assert!(saw_completed);
}
```

- [ ] **Step 7: Run the focused Python forwarding test and confirm GREEN.**

```powershell
cargo test -p control-center-core python_config_job_forwards_discoveries_and_counts_them --locked -j 1 -- --nocapture
```

Expected: PASS without requiring `runtimes\cpython-3.14.7-windows-x86_64\python.exe`.

- [ ] **Step 8: Write the isolated missing-root test.** It must prove the Python runner is never invoked, `python.config` emits `scanner_failed`, another job settles normally and the global scan still terminates:

```rust
#[cfg(indows)]
#[tokio::test]
async fn missing_python_root_fails_only_python_job_and_scan_still_settles() {
    let python_job = build_python_config_job_with_runner(
        Err(PythonRootError),
        Vec::new(),
        |_app_root, _roots, _timeout, _cancellation, _discoveries| async move {
            panic!("Python runner must not execute when the application root is unavailable");
            #[allow(unreachable_code)]
            Ok::<u64, crate::python_supervisor::PythonSupervisorError>(0)
        },
    );

    let native_job = ScannerJob::new(
        "test.native",
        Duration::from_secs(1),
        |_events, _cancellation| async move {
            ScannerTerminal::Completed {
                visited: 2,
                discovered: 1,
            }
        },
    );

    let (sender, mut receiver) = mpsc::channel(16);
    run_scanner_jobs(
        vec![python_job, native_job],
        2,
        sender,
        CancellationToken::new(),
    )
    .await;

    let mut saw_python_failure = false;
    let mut saw_completed = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            ScanEvent::ScannerFailed {
                scanner_id, code, ..
            } if scanner_id == "python.config" && code == "scanner_failed" => {
                saw_python_failure = true;
            }
            ScanEvent::Completed {
                visited: 2,
                discovered: 1,
            } => saw_completed = true,
            _ => {}
        }
    }

    assert!(saw_python_failure);
    assert!(saw_completed);
}
```

- [ ] **Step 9: Run the missing-root test and confirm GREEN.**

```powershell
cargo test -p control-center-core missing_python_root_fails_only_python_job_and_scan_still_settles --locked -j 1 -- --nocapture
```

Expected: PASS.

- [ ] **Step 10: Run all core scan tests.**

```powershell
cargo test -p control-center-core scan::tests --locked -j 1 -- --nocapture
```

Expected: all existing coordinator tests plus the new Python tests PASS.

- [ ] **Step 11: Commit the core integration.**

```powershell
git add engine/rust/control-center-core/src/scan.rs engine/rust/control-center-core/src/lib.rs
git commit -m "feat: integrate Python scanner into quick scan"
```

### Task 2: Resolve the Python application root in the Tauri shell

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_root.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: inline tests in `apps/desktop/src-tauri/src/runtime_root.rs`

**Interfaces:**

- Consumes: `control_center_core::PythonRootError` and `control_center_core::QuickScanContext`.
- Produces: `pub(crate) fn resolve_python_app_root() -> Result<PathBuf, PythonRootError>`.
- Produces internally: `development_app_root(manifest_dir: &Path) -> Result<PathBuf, PythonRootError>` and `packaged_app_root(executable: &Path) -> Result<PathBuf, PythonRootError>`.

- [ ] **Step 1: Create failing deterministic-root tests.** Create `runtime_root.rs` with the tests first. Use `std::env::temp_dir()` plus the already-present `uuid` dependency, so no new test dependency is needed:

```rust
use control_center_core::PythonRootError;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ai-tool-control-runtime-root-test-{}",
            Uuid::new_v4()
        ));
        fs:create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn development_root_is_repository_root_and_absolute() {
        let root = temporary_root();
        let manifest_dir = root.join("apps").join("desktop").join("src-tauri");
        fs:create_dir_all(&manifest_dir).unwrap();

        let resolved = development_app_root(&manifest_dir).unwrap();
        let expected = root.canonicalize().unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, expected);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packaged_root_is_executable_parent_and_absolute() {
        let root = temporary_root();
        let executable = root.join("AI Tool Control Center.exe");

        let resolved = packaged_app_root(&executable).unwrap();
        let expected = root.canonicalize().unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, expected);

        fs::remove_dir_all(root).unwrap();
    }
}
```

- [ ] **Step 2: Register the module and run the desktop tests to confirm RED.** Add only this line near the top of `apps/desktop/src-tauri/src/lib.rs`:

```rust
mod runtime_root;
```

Then run:

```powershell
cargo test -p ai-tool-control-center-desktop runtime_root --locked -j 1 -- --nocapture
```

Expected: compile failure because `development_app_root` and `packaged_app_root` do not exist yet.


- [ ] **Step 3: Implement deterministic development and packaged root helpers.** Keep both helpers independent of Tauri so they can be unit tested directly:

```rust
fn absolute_existing_dir(path: &Path) -> Result<PathBuf, PythonRootError> {
    let canonical = path.canonicalize().map_err(|_| PythonRootError)?;
    if canonical.is_absolute() {
        Ok(canonical)
    } else {
        Err(PythonRootError)
    }
}

fn development_app_root(manifest_dir: &Path) -> Result<PathBuf, PythonRootError> {
    absolute_existing_dir(&manifest_dir.join("..").join("..").join(".."))
}

fn packaged_app_root(executable: &Path) -> Result<PathBuf, PythonRootError> {
    if !executable.is_absolute() {
        return Err(PythonRootError);
    }

    let parent = executable.parent().ok_or(PythonRootError)?;
    absolute_existing_dir(parent)
}

pub(crate) fn resolve_python_app_root() -> Result<PathBuf, PythonRootError> {
    #[cfg(debug_assertions)]
    {
        development_app_root(Path::new(env(!("CARGO_MANIFEST_DIR")))
    }

    #[cfg(not(debug_assertions))]
    {
        let executable = std::env::current_exe().map_err(|_| PythonRootError)?;
        packaged_app_root(&executable)
    }
}
```

Do not inspect `PATH` and do not look for any interpreter here. This module resolves only the application root. `python_supervisor::resolve_staged_python` remains the sole interpreter-path resolver.

- [ ] **Step 4: Run the focused root tests and confirm GREEN.**

```powershell
cargo test -p ai-tool-control-center-desktop runtime_root --locked -j 1 -- --nocapture
```

Expected: both deterministic root tests PASS.

- [ ] **Step 5: Wire `QuickScanContext` into the Tauri command.** Register the module and import the new core type:

```rust
mod runtime_root;

use control_center_core::{
    Discovery, QuickScanContext, ReviewDecision, ScanEvent, Store, quick_scan,
};
```

In `start_quick_scan`, after validating the two Windows discovery roots, build the context without converting Python root failure into a command error:

```rust
let context = QuickScanContext {
    roots,
    python_app_root: runtime_root::resolve_python_app_root(),
};
```

Move `context` into the existing scan task:

```rust
tauri::async_runtime::spawn(async move {
    quick_scan(context, sender, cancellation).await;
});
```

Do not change the receiver task. Its existing `ScanEvent::Discovery` branch must continue persisting every discovery before emitting it to the frontend, including discoveries whose `source_scanner` is `python.config`.

- [ ] **Step 6: Run the desktop crate tests and compile check.**

```powershell
cargo test -p ai-tool-control-center-desktop --locked -j 1 -- --nocapture
```

Expected: PASS.

```powershell
cargo check -p ai-tool-control-center-desktop --locked
```

Expected: PASS.

- [ ] **Step 7: Commit the shell integration.**

```powershell
git add apps/desktop/src-tauri/src/runtime_root.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: resolve bundled Python root for quick scan"
```

### Task 3: Verify failure isolation, cancellation semantics and the complete Windows slice

**Files:**

- Modify only if a focused regression test exposes a defect: `engine/rust/control-center-core/src/scan.rs`
- Verification only: `engine/rust/control-center-core/src/python_supervisor.rs`
- Verification only: `apps/desktop/src-tauri/src/lib.rs`
- Verification only: `packaging/test-runtime-smoke.ps1`
- Verification only: `.github/workflows/ci.yml`

**Interfaces:**

- Consumes: `quick_scan(QuickScanContext, mpsc::Sender<ScanEvent>, CancellationToken)`.
- Consumes: stable scanner code mapping from `PythonSupervisorError::code()`.
- Produces: verified Windows Quick Scan behavior with eight settled jobs and no ambient-Python dependency.

- [ ] **Step 1: Add a focused cancellation-mapping regression test only for the new adapter boundary.** This test must not recreate supervisor process-tree or protocol tests. Inject a runner that emits one discovery and then returns `PythonSupervisorError::cancelled()`:

```rust
#[cfg(windows)]
#[tokio::test]
async fn python_config_cancellation_preserves_forwarded_discovery_and_scan_settlement() {
    let mut discovery =
        Discovery::unknown("Python config", "python.config", "python-config-cancel-test");
    discovery.evidence.push(Evidence {
        kind: "config".into(),
        summary: "test config".into(),
    });

    let job = build_python_config_job_with_runner(
        Ok(PathBuf::from(r"C:\app")),
        Vec::new(),
        move |_app_root, _roots, _timeout, _cancellation, discoveries| async move {
            discoveries.send(discovery).unwrap();
            Err(crate::python_supervisor::PythonSupervisorError::cancelled())
        },
     );

    let native_job = ScannerJob::new(
        "test.native",
        Duration::from_secs(1),
        |_events, _cancellation| async move {
            ScannerTerminal::Completed {
                visited: 2,
                discovered: 1,
            }
        },
     );

    let (sender, mut receiver) = mpsc::channel(16);
    run_scanner_jobs(
        vec![job, native_job],
        2,
        sender,
        CancellationToken::new(),
    )
    .await;

    let mut saw_python_discovery = false;
    let mut saw_cancelled = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            ScanEvent::Discovery { discovery }
                if discovery.source_scanner == "python.config" =>
            {
                saw_python_discovery = true;
            }
            ScanEvent::Cancelled {
                visited: 2,
                discovered: 2,
            } => saw_cancelled = true,
            _ => {}
        }
    }

    assert!(saw_python_discovery);
    assert!(saw_cancelled);
}
```

This test covers only the newly introduced translation from supervisor cancellation into coordinator cancellation. Do not add another timeout-process test: `ScannerJob::run` already owns coordinator timeout and existing supervisor tests own process timeout cleanup.

- [ ] **Step 2: Run the cancellation regression test and confirm GREEN after the Task 1 adapter exists.**

```powershell
cargo test -p control-center-core python_config_cancellation_preserves_forwarded_discovery_and_scan_settlement --locked -j 1 -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run Rust formatting and diff hygiene.**

```powershell
cargo fmt --all -- --check
```

Expected: PASS.

```powershell
git diff --check
```

Expected: no output.

- [ ] **Step 4: Run Clippy exactly as the release gate.**

```powershell
cargo clippy --workspace --all-targets --locked -j 1 -- -D warnings
```

Expected: PASS with no warnings.

- [ ] **Step 5: Run the full Rust workspace tests serially.**

```powershell
cargo test --workspace --locked -j 1 -- --test-threads=1
```

Expected: PASS. These tests must pass even when `runtimes/cpython-3.14.7-windows-x86_64/python.exe` is absent because unit tests use the injected Python runner.

- [ ] **Step 6: Run Python verification.**

```powershell
uv sync --project engine/python --frozen
```

Expected: dependencies synchronize successfully.

```powershell
uv run --project engine/python ruff check engine/python
```

Expected: PASS.

```powershell
uv run --project engine/python mypy engine/python/src
```

Expected: PASS.

```powershell
$env:PYTHONPATH="engine/python/src"; python -m unittest discover -s engine/python/tests -v
```

Expected: PASS.

- [ ] **Step 7: Run frontend verification.**

```powershell
pnpm install --frozen-lockfile
```

Expected: PASS.

```powershell
pnpm lint
```

Expected: PASS.

```powershell
pnpm test
```

Expected: PASS.

```powershell
pnpm build
```

Expected: PASS.


### Task 4: Add real Windows acceptance coverage and run every release gate

**Files:**

- Modify: `engine/rust/control-center-core/src/scan.rs` only for the two ignored Windows acceptance tests described below.
- Verification only: `engine/rust/control-center-core/src/python_supervisor.rs`.
- Verification only: `apps/desktop/src-tauri/src/lib.rs`.
- Verification only: `packaging/test-runtime-smoke.ps1`.
- Verification only: `packaging/verify-artifacts.ps1`.
- Verification only: `.github/workflows/ci.yml`.

**Interfaces:**

- Consumes: private production helper `build_python_config_job(Result<PathBuf, PythonRootError>, Vec<PathBuf>) -> ScannerJob`.
- Consumes: `run_scanner_jobs(Vec<ScannerJob>, usize, mpsc::Sender<ScanEvent>, CancellationToken)`.
- Produces: ignored Windows test `bundled_python_config_job_runs_with_staged_runtime`.
- Produces: ignored Windows test `missing_bundled_python_runtime_fails_only_python_job`.
- Produces no new production API.

- [ ] **Step 1: Add an ignored real-runtime acceptance test.** The test must use the repository-owned staged runtime through the production Python job, not an injected runner. Derive the repository root from the core crate compile-time manifest path so the test is independent of the current working directory:

```rust
#[cfg(windows)]
fn repository_root_for_acceptance() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root must exist")
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires runtimes/cpython-3.14.7-windows-x86_64/python.exe"]
async fn bundled_python_config_job_runs_with_staged_runtime() {
    let app_root = repository_root_for_acceptance();
    let job = build_python_config_job(Ok(app_root), Vec::new());

    let (sender, mut receiver) = mpsc::channel(16);
    run_scanner_jobs(vec![job], 1, sender, CancellationToken::new()).await;

    let mut saw_completed = false;
    let mut saw_python_failure = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            ScanEvent::ScannerFailed { scanner_id, .. }
                if scanner_id == "python.config" =>
            {
                saw_python_failure = true;
            }
            ScanEvent::Completed { .. } => saw_completed = true,
            _ => {}
        }
    }

    assert!(!saw_python_failure);
    assert!(saw_completed);
}
```

The runtime may legitimately discover zero configurations when the roots list is empty. This acceptance test proves only that the production coordinator job launches the staged interpreter and settles successfully.

- [ ] **Step 2: Run the real-runtime acceptance test with the bundled runtime present.**

```powershell
Test-Path "runtimes\cpython-3.14.7-windows-x86_64\python.exe"
```

Expected: `True`.

```powershell
cargo test -p control-center-core bundled_python_config_job_runs_with_staged_runtime --locked -j 1 -- --ignored --nocapture
```

Expected: PASS and no `python.config` scanner failure.

- [ ] **Step 3: Add an ignored missing-runtime isolation acceptance test.** This test intentionally expects the production Python job to fail while a native control job succeeds:

```rust
#[cfg(windows)]
#[tokio::test]
#[ignore = "run only while the staged bundled runtime directory is temporarily unavailable"]
async fn missing_bundled_python_runtime_fails_only_python_job() {
    let app_root = repository_root_for_acceptance();
    let python_job = build_python_config_job(Ok(app_root), Vec::new());
    let native_job = ScannerJob::new(
        "test.native",
        Duration::from_secs(1),
        |_events, _cancellation| async move {
            ScannerTerminal::Completed {
                visited: 2,
                discovered: 1,
            }
        },
    );

    let (sender, mut receiver) = mpsc::channel(16);
    run_scanner_jobs(
        vec![python_job, native_job],
        2,
        sender,
        CancellationToken::new(),
    )
    .await;

    let mut saw_python_failure = false;
    let mut saw_completed = false;

    while let Ok(event) = receiver.try_recv() {
        match event {
            ScanEvent::ScannerFailed {
                scanner_id, code, ..
            } if scanner_id == "python.config" && code == "scanner_failed" => {
                saw_python_failure = true;
            }
            ScanEvent::Completed {
                visited: 2,
                discovered: 1,
            } => saw_completed = true,
            _ => {}
        }
    }

    assert!(saw_python_failure);
    assert!(saw_completed);
}
```

Do not change `resolve_staged_python` for this test and do not add an ambient-Python fallback.

- [ ] **Step 4: Temporarily make the staged runtime unavailable and run the isolated-failure acceptance test.** First rename the runtime directory, run the test, then restore it immediately:

```powershell
Rename-Item "runtimes\cpython-3.14.7-windows-x86_64" "cpython-3.14.7-windows-x86_64.acceptance-disabled"
```

Expected: no output.

```powershell
cargo test -p control-center-core missing_bundled_python_runtime_fails_only_python_job --locked -j 1 -- --ignored --nocapture
```

Expected: PASS. `python.config` reports `scanner_failed`, `test.native` still completes and the global scan emits `Completed { visited: 2, discovered: 1 }`.

```powershell
Rename-Item "runtimes\cpython-3.14.7-windows-x86_64.acceptance-disabled" "cpython-3.14.7-windows-x86_64"
```

Expected: no output.

```powershell
Test-Path "runtimes\cpython-3.14.7-windows-x86_64\python.exe"
```

Expected: `True`. If the acceptance test itself fails, restore the directory before any debugging or rerun.

- [ ] **Step 5: Run the existing bundled-runtime smoke with ambient Python poisoned.**

```powershell
pwsh -File packaging/test-runtime-smoke.ps1
```

Expected final line: `PASS: verify-artifacts proves bundled CPython works without ambient Python.`

- [ ] **Step 6: Run repository policy and Tauri release compilation.**

```powershell
python scripts/repository_policy.py .
```

Expected: PASS.

```powershell
pnpm tauri build --no-bundle
```

Expected: PASS. This is a release compile only. Do not add final NSIS runtime-resource packaging in this slice.

- [ ] **Step 7: Re-run the complete verification matrix after the ignored acceptance tests are added.**

```powershell
cargo fmt --all -- --check
```

Expected: PASS.

```powershell
cargo clippy --workspace --all-targets --locked -j 1 -- -D warnings
```

Expected: PASS with no warnings.

```powershell
cargo test --workspace --locked -j 1 -- --test-threads=1
```

Expected: PASS. The two real-runtime acceptance tests remain ignored during the ordinary workspace run.

```powershell
uv run --project engine/python ruff check engine/python
```

Expected: PASS.

```powershell
uv run --project engine/python mypy engine/python/src
```

Expected: PASS.

```powershell
$env:PYTHONPATH="engine/python/src"; python -m unittest discover -s engine/python/tests -v
```

Expected: PASS.

```powershell
pnpm lint
```

Expected: PASS.

```powershell
pnpm test
```

Expected: PASS.

````powershell
pnpm build
```

Expected: PASS.

````powershell
python scripts/repository_policy.py .
```

Expected: PASS.

````powershell
pnpm tauri build --no-bundle
```

Expected: PASS.

````powershell
git diff --check
```

Expected: no output.

```powershell
pwsh -File packaging/test-runtime-smoke.ps1
```

Expected: PASS.

- [ ] **Step 8: Inspect the final diff for scope discipline.**

```powershell
git status --short
```

Expected modified production files are limited to the Quick Scan core and desktop runtime-root integration, plus this plan while it remains uncommitted.

```powershell
git diff --stat
```

Expected: no frontend redesign, no installer-resource packaging and no version bump.
