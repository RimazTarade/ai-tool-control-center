use crate::scan_control::{PauseGate, ScanEvent, ScanEventSink, ScanScope};
use crate::{Discovery, Evidence};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use walkdir::{DirEntry, WalkDir};

pub use crate::scan_control::QUICK_SCAN_CONCURRENCY;

const EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "target",
    "AppData",
    "Cache",
    "Caches",
    ".cache",
    "cache2",
    "Code Cache",
    "GPUCache",
    "INetCache",
    "LocalCache",
    "Crashpad",
    "CrashDumps",
    "Temp",
    "tmp",
    "npm-cache",
    "pip-cache",
];
const SIGNALS: &[&str] = &[
    "mcp",
    "claude",
    "codex",
    "ollama",
    "openwebui",
    "n8n",
    "docker",
];

const QUICK_SCAN_SCANNER_IDS: &[&str] = &[
    "filesystem.quick",
    "windows.known_location",
    "windows.path",
    "windows.uninstall_registry",
    "windows.process",
    "windows.service",
    "windows.tcp",
    "python.config",
];

fn quick_scan_scanner_ids() -> &'static [&'static str] {
    QUICK_SCAN_SCANNER_IDS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PythonRootError;

#[derive(Debug, Clone)]
pub struct QuickScanContext {
    pub roots: Vec<PathBuf>,
    pub python_app_root: Result<PathBuf, PythonRootError>,
}

#[derive(Debug)]
struct ScanSettlement {
    pending: std::collections::HashSet<String>,
}

impl ScanSettlement {
    fn new<I, S>(scanner_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            pending: scanner_ids.into_iter().map(Into::into).collect(),
        }
    }

    fn mark_settled(&mut self, scanner_id: &str) -> bool {
        self.pending.remove(scanner_id)
    }

    fn is_terminal(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Debug)]
enum ScannerTerminal {
    Completed { visited: u64, discovered: u64 },
    Cancelled { visited: u64, discovered: u64 },
    Failed { code: String, message: String },
}

type ScannerFuture = Pin<Box<dyn Future<Output = ScannerTerminal> + Send>>;
type ScannerRunner =
    Box<dyn FnOnce(ScanEventSink, CancellationToken) -> ScannerFuture + Send>;

struct ScannerJob {
    scanner_id: String,
    timeout: Duration,
    runner: ScannerRunner,
}

impl ScannerJob {
    fn new<F, Fut>(scanner_id: impl Into<String>, timeout: Duration, runner: F) -> Self
    where
        F: FnOnce(ScanEventSink, CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = ScannerTerminal> + Send + 'static,
    {
        Self {
            scanner_id: scanner_id.into(),
            timeout,
            runner: Box::new(move |events, cancellation| Box::pin(runner(events, cancellation))),
        }
    }

    async fn run(self, events: ScanEventSink, cancellation: CancellationToken) -> ScannerTerminal {
        let timeout = self.timeout;
        let timeout_cancellation = cancellation.clone();
        let runner = (self.runner)(events, cancellation);

        match tokio::time::timeout(timeout, runner).await {
            Ok(terminal) => terminal,
            Err(_) => {
                timeout_cancellation.cancel();
                ScannerTerminal::Failed {
                    code: "scanner_timeout".into(),
                    message: "Scanner exceeded its execution timeout".into(),
                }
            }
        }
    }
}

async fn run_scanner_jobs(
    jobs: Vec<ScannerJob>,
    max_concurrency: usize,
    events: ScanEventSink,
    pause_gate: PauseGate,
    cancellation: CancellationToken,
) {
    let scanner_ids: Vec<String> = jobs.iter().map(|job| job.scanner_id.clone()).collect();
    let mut coordinator = ScanCoordinatorState::new(scanner_ids, cancellation.clone());
    let semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
    let mut tasks = JoinSet::new();

    for job in jobs {
        let scanner_id = job.scanner_id.clone();
        let semaphore = semaphore.clone();
        let job_events = events.clone();
        let child_cancellation = cancellation.child_token();
        let pause_gate = pause_gate.clone();

        tasks.spawn(async move {
            let permit = tokio::select! {
                _ = child_cancellation.cancelled() => {
                    return (
                        scanner_id,
                        ScannerTerminal::Cancelled {
                            visited: 0,
                            discovered: 0,
                        },
                    );
                }
                permit = semaphore.acquire_owned() => {
                    permit.expect("scanner concurrency semaphore closed unexpectedly")
                }
            };

            if child_cancellation.is_cancelled() {
                drop(permit);
                return (
                    scanner_id,
                    ScannerTerminal::Cancelled {
                        visited: 0,
                        discovered: 0,
                    },
                );
            }

            // Cooperative pause: a queued job must not start its bounded
            // operation while paused. Cancellation still wins the race.
            if !pause_gate.checkpoint(&child_cancellation).await {
                drop(permit);
                return (
                    scanner_id,
                    ScannerTerminal::Cancelled {
                        visited: 0,
                        discovered: 0,
                    },
                );
            }

            job_events.progress(ScanEvent::Progress {
                scanner_id: scanner_id.clone(),
                completed_units: 0,
                total_units: None,
                current_location: None,
            });

            let scanner_task = tokio::spawn(job.run(job_events, child_cancellation));
            let terminal = match scanner_task.await {
                Ok(terminal) => terminal,
                Err(_) => ScannerTerminal::Failed {
                    code: "scanner_failed".into(),
                    message: "Scanner task stopped unexpectedly".into(),
                },
            };

            (scanner_id, terminal)
        });
    }

    while let Some(result) = tasks.join_next().await {
        let Ok((scanner_id, terminal)) = result else {
            continue;
        };

        coordinator.settle(&scanner_id, terminal, &events).await;
    }
}

const QUICK_SCAN_SCANNER_TIMEOUT: Duration = Duration::from_secs(60);

async fn emit_discoveries(
    events: ScanEventSink,
    cancellation: CancellationToken,
    discoveries: Vec<Discovery>,
) -> ScannerTerminal {
    let mut discovered = 0;

    for discovery in discoveries {
        if cancellation.is_cancelled() {
            return ScannerTerminal::Cancelled {
                visited: 0,
                discovered,
            };
        }

        if events
            .critical(ScanEvent::Discovery { discovery })
            .await
            .is_err()
        {
            return ScannerTerminal::Failed {
                code: "event_channel_closed".into(),
                message: "Scan event channel closed while publishing discoveries".into(),
            };
        }

        discovered += 1;
    }

    ScannerTerminal::Completed {
        visited: 0,
        discovered,
    }
}

async fn emit_partial_failure(events: &ScanEventSink, scanner_id: &str, message: String) {
    let _ = events
        .scanner_failed(scanner_id.to_string(), "partial_failure", message)
        .await;
}

#[cfg(windows)]
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
                            if events
                                .critical(ScanEvent::Discovery { discovery })
                                .await
                                .is_err()
                            {
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
                                if events
                                    .critical(ScanEvent::Discovery { discovery })
                                    .await
                                    .is_err()
                                {
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
#[cfg(windows)]
fn build_quick_scan_jobs(context: QuickScanContext) -> Vec<ScannerJob> {
    use crate::windows::{
        WindowsProcessSource, WindowsServiceSource, WindowsTcpEndpointSource,
        WindowsUninstallRegistrySource, discover_known_locations, discover_path_executables,
        discover_processes_with, discover_services_with, discover_tcp_endpoints_with,
        discover_uninstall_registry_with, windows_known_location_roots,
    };
    let python_roots = context.roots.clone();
    let roots = context.roots;
    let python_app_root = context.python_app_root;

    let path_value = std::env::var("PATH").unwrap_or_default();
    let pathext_value = std::env::var("PATHEXT").unwrap_or_default();

    let mut jobs = vec![
        ScannerJob::new(
            "filesystem.quick",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                match tokio::task::spawn_blocking(move || {
                    scan_blocking(roots, events, cancellation)
                })
                .await
                {
                    Ok(terminal) => terminal,
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "The filesystem scanner stopped unexpectedly".into(),
                    },
                }
            },
        ),
        ScannerJob::new(
            "windows.known_location",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    let root_report = windows_known_location_roots();
                    let report = discover_known_locations(&root_report.roots, 5);
                    (root_report.errors, report)
                });

                let (mut errors, report) = match task.await {
                    Ok(result) => result,
                    Err(_) => {
                        return ScannerTerminal::Failed {
                            code: "scanner_failed".into(),
                            message: "Windows known-location scanner stopped unexpectedly".into(),
                        };
                    }
                };

                errors.extend(
                    report
                        .errors
                        .into_iter()
                        .map(|error| format!("{}: {}", error.root.display(), error.message)),
                );

                if !errors.is_empty() {
                    emit_partial_failure(&events, "windows.known_location", errors.join("; "))
                        .await;
                }

                emit_discoveries(events, cancellation, report.discoveries).await
            },
        ),
        ScannerJob::new(
            "windows.path",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    discover_path_executables(&path_value, &pathext_value)
                });

                match task.await {
                    Ok(discoveries) => emit_discoveries(events, cancellation, discoveries).await,
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "Windows PATH scanner stopped unexpectedly".into(),
                    },
                }
            },
        ),
        ScannerJob::new(
            "windows.uninstall_registry",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    discover_uninstall_registry_with(&WindowsUninstallRegistrySource)
                });

                let report = match task.await {
                    Ok(report) => report,
                    Err(_) => {
                        return ScannerTerminal::Failed {
                            code: "scanner_failed".into(),
                            message: "Windows uninstall-registry scanner stopped unexpectedly"
                                .into(),
                        };
                    }
                };

                if !report.errors.is_empty() {
                    let message = report
                        .errors
                        .iter()
                        .map(|error| {
                            format!("{:?} {:?}: {}", error.hive, error.view, error.message)
                        })
                        .collect::<Vec<_>>()
                        .join("; ");

                    emit_partial_failure(&events, "windows.uninstall_registry", message).await;
                }

                emit_discoveries(events, cancellation, report.discoveries).await
            },
        ),
        ScannerJob::new(
            "windows.process",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    discover_processes_with(&WindowsProcessSource)
                });

                match task.await {
                    Ok(Ok(discoveries)) => {
                        emit_discoveries(events, cancellation, discoveries).await
                    }
                    Ok(Err(message)) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message,
                    },
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "Windows process scanner stopped unexpectedly".into(),
                    },
                }
            },
        ),
        ScannerJob::new(
            "windows.service",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    discover_services_with(&WindowsServiceSource)
                });

                match task.await {
                    Ok(Ok(discoveries)) => {
                        emit_discoveries(events, cancellation, discoveries).await
                    }
                    Ok(Err(message)) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message,
                    },
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "Windows service scanner stopped unexpectedly".into(),
                    },
                }
            },
        ),
        ScannerJob::new(
            "windows.tcp",
            QUICK_SCAN_SCANNER_TIMEOUT,
            move |events, cancellation| async move {
                let task = tokio::task::spawn_blocking(move || {
                    discover_tcp_endpoints_with(&WindowsTcpEndpointSource)
                });

                match task.await {
                    Ok(Ok(discoveries)) => {
                        emit_discoveries(events, cancellation, discoveries).await
                    }
                    Ok(Err(message)) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message,
                    },
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "Windows TCP scanner stopped unexpectedly".into(),
                    },
                }
            },
        ),
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

#[derive(Debug)]
struct ScanCoordinatorState {
    settlement: ScanSettlement,
    visited: u64,
    discovered: u64,
    cancelled: bool,
    start: Instant,
    cancellation: CancellationToken,
}

impl ScanCoordinatorState {
    fn new<I, S>(scanner_ids: I, cancellation: CancellationToken) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            settlement: ScanSettlement::new(scanner_ids),
            visited: 0,
            discovered: 0,
            cancelled: false,
            start: Instant::now(),
            cancellation,
        }
    }

    /// Records a scanner's terminal outcome. Emits (via `events`) a
    /// `ScannerFailed` event immediately on failure, and once every
    /// scanner has settled, emits the final `Completed`/`Cancelled` event
    /// carrying the accumulated failure count and elapsed duration.
    /// Cancellation of the root token wins a race with child settlement
    /// when choosing the final terminal state.
    async fn settle(&mut self, scanner_id: &str, terminal: ScannerTerminal, events: &ScanEventSink) {
        if !self.settlement.mark_settled(scanner_id) {
            return;
        }

        match terminal {
            ScannerTerminal::Completed {
                visited,
                discovered,
            } => {
                self.visited = self.visited.saturating_add(visited);
                self.discovered = self.discovered.saturating_add(discovered);
            }
            ScannerTerminal::Cancelled {
                visited,
                discovered,
            } => {
                self.visited = self.visited.saturating_add(visited);
                self.discovered = self.discovered.saturating_add(discovered);
                self.cancelled = true;
            }
            ScannerTerminal::Failed { code, message } => {
                let _ = events.scanner_failed(scanner_id.to_string(), code, message).await;
            }
        }

        if self.is_terminal() {
            let cancelled = self.cancelled || self.cancellation.is_cancelled();
            let failure_count = events.failure_count();
            let duration_ms = self.start.elapsed().as_millis() as u64;

            let terminal_event = if cancelled {
                ScanEvent::Cancelled {
                    visited: self.visited,
                    discovered: self.discovered,
                    failure_count,
                    duration_ms,
                }
            } else {
                ScanEvent::Completed {
                    visited: self.visited,
                    discovered: self.discovered,
                    failure_count,
                    duration_ms,
                }
            };

            let _ = events.critical(terminal_event).await;
        }
    }

    fn is_terminal(&self) -> bool {
        self.settlement.is_terminal()
    }
}

pub async fn quick_scan(
    context: QuickScanContext,
    events: ScanEventSink,
    pause_gate: PauseGate,
    cancellation: CancellationToken,
) {
    let _ = events
        .critical(ScanEvent::Started {
            scope: ScanScope::Quick,
            scanner_count: QUICK_SCAN_SCANNER_IDS.len(),
        })
        .await;

    #[cfg(windows)]
    {
        let jobs = build_quick_scan_jobs(context);
        run_scanner_jobs(jobs, QUICK_SCAN_CONCURRENCY, events, pause_gate, cancellation).await;
    }

    #[cfg(not(windows))]
    {
        const SCANNER_ID: &str = "filesystem.quick";

        let roots = context.roots;
        let terminal_events = events.clone();
        let mut coordinator = ScanCoordinatorState::new([SCANNER_ID], cancellation.clone());

        if !pause_gate.checkpoint(&cancellation).await {
            coordinator
                .settle(
                    SCANNER_ID,
                    ScannerTerminal::Cancelled {
                        visited: 0,
                        discovered: 0,
                    },
                    &terminal_events,
                )
                .await;
            return;
        }

        let task = tokio::task::spawn_blocking(move || scan_blocking(roots, events, cancellation));

        let terminal = match task.await {
            Ok(terminal) => terminal,
            Err(_) => ScannerTerminal::Failed {
                code: "scanner_failed".into(),
                message: "The filesystem scanner stopped unexpectedly".into(),
            },
        };

        coordinator.settle(SCANNER_ID, terminal, &terminal_events).await;
    }
}

fn scan_blocking(
    roots: Vec<PathBuf>,
    events: ScanEventSink,
    cancellation: CancellationToken,
) -> ScannerTerminal {
    let mut visited = 0;
    let mut discovered = 0;
    for root in roots {
        if cancellation.is_cancelled() {
            return ScannerTerminal::Cancelled {
                visited,
                discovered,
            };
        }
        if !root.is_absolute() || !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(5)
            .into_iter()
            .filter_entry(is_allowed)
            .filter_map(Result::ok)
        {
            if cancellation.is_cancelled() {
                return ScannerTerminal::Cancelled {
                    visited,
                    discovered,
                };
            }
            visited += 1;
            if visited % 64 == 0 {
                events.progress(ScanEvent::Progress {
                    scanner_id: "filesystem.quick".into(),
                    completed_units: visited,
                    total_units: None,
                    current_location: None,
                });
            }
            if entry.file_type().is_file() && looks_relevant(entry.path()) {
                discovered += 1;
                let path = entry.path().to_path_buf();
                let mut discovery = Discovery::unknown(
                    entry.file_name().to_string_lossy(),
                    "filesystem.quick",
                    fingerprint(&path),
                );
                discovery.evidence.push(Evidence {
                    kind: "path".into(),
                    summary: path.display().to_string(),
                });
                let _ = events.blocking_critical(ScanEvent::Discovery { discovery });
            }
        }
    }
    ScannerTerminal::Completed {
        visited,
        discovered,
    }
}

fn is_allowed(entry: &DirEntry) -> bool {
    entry.depth() == 0
        || !EXCLUDED.iter().any(|excluded| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(excluded)
        })
}

fn looks_relevant(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    SIGNALS.iter().any(|signal| name.contains(signal))
        && matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("json" | "yaml" | "yml" | "toml" | "exe" | "cmd" | "bat" | "ps1")
        )
}

fn fingerprint(path: &Path) -> String {
    Sha256::digest(path.to_string_lossy().to_ascii_lowercase().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn quick_scan_exposes_stable_scanner_ids() {
        assert_eq!(
            quick_scan_scanner_ids(),
            &[
                "filesystem.quick",
                "windows.known_location",
                "windows.path",
                "windows.uninstall_registry",
                "windows.process",
                "windows.service",
                "windows.tcp",
                "python.config",
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn quick_scan_builds_every_stable_scanner_job() {
        let jobs = build_quick_scan_jobs(QuickScanContext {
            roots: Vec::new(),
            python_app_root: Err(PythonRootError),
        });
        let scanner_ids: Vec<&str> = jobs.iter().map(|job| job.scanner_id.as_str()).collect();

        assert_eq!(scanner_ids, quick_scan_scanner_ids());
    }

    #[tokio::test]
    async fn quick_scan_emits_started_with_public_scope_and_scanner_count() {
        let (sender, mut receiver) = mpsc::channel(64);
        let events = ScanEventSink::new(sender);

        let context = QuickScanContext {
            roots: Vec::new(),
            python_app_root: Err(PythonRootError),
        };

        // Cancel immediately so any real scanner jobs (windows registry,
        // process, service, tcp, ...) short-circuit rather than running
        // against the live system. We only care that `Started` is the
        // first event on the wire, emitted before any scanner starts.
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let scan_task = tokio::spawn(quick_scan(
            context,
            events,
            PauseGate::default(),
            cancellation,
        ));

        let first = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for first scan event")
            .expect("expected at least one event");

        match first {
            ScanEvent::Started {
                scope,
                scanner_count,
            } => {
                assert_eq!(scope, ScanScope::Quick);
                assert_eq!(scanner_count, 8);
            }
            other => panic!("expected ScanEvent::Started first, got {other:?}"),
        }

        scan_task.abort();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn python_config_job_forwards_discoveries_and_counts_them() {
        let app_root = tempdir().unwrap();
        let expected_root = app_root.path().to_path_buf();
        let expected_root_for_runner = expected_root.clone();

        let mut discovery = Discovery::unknown(
            "Python config",
            "python.config",
            "python-config-test".into(),
        );
        discovery.evidence.push(Evidence {
            kind: "config".into(),
            summary: "test config".into(),
        });

        let job = build_python_config_job_with_runner(
            Ok(expected_root),
            vec![PathBuf::from(r"C:\scan-root")],
            move |app_root, roots, _timeout, _cancellation, discoveries| async move {
                assert_eq!(app_root, expected_root_for_runner);
                assert_eq!(roots, vec![PathBuf::from(r"C:\scan-root")]);
                discoveries.send(discovery).unwrap();
                Ok::<u64, crate::python_supervisor::PythonSupervisorError>(1)
            },
        );

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);
        run_scanner_jobs(vec![job], 1, events, PauseGate::default(), CancellationToken::new()).await;

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
                ScanEvent::Completed { discovered: 1, .. } => saw_completed = true,
                _ => {}
            }
        }

        assert!(saw_discovery);
        assert!(saw_completed);
    }

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
        let events = ScanEventSink::new(sender);
        run_scanner_jobs(vec![job], 1, events, PauseGate::default(), CancellationToken::new()).await;

        let mut saw_completed = false;
        let mut saw_python_failure = false;

        while let Ok(event) = receiver.try_recv() {
            match event {
                ScanEvent::ScannerFailed { scanner_id, .. } if scanner_id == "python.config" => {
                    saw_python_failure = true;
                }
                ScanEvent::Completed { .. } => saw_completed = true,
                _ => {}
            }
        }

        assert!(!saw_python_failure);
        assert!(saw_completed);
    }
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
        let events = ScanEventSink::new(sender);
        run_scanner_jobs(
            vec![python_job, native_job],
            2,
            events,
            PauseGate::default(),
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
                    ..
                } => saw_completed = true,
                _ => {}
            }
        }

        assert!(saw_python_failure);
        assert!(saw_completed);
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn python_config_cancellation_preserves_forwarded_discovery_and_scan_settlement() {
        let mut discovery = Discovery::unknown(
            "Python config",
            "python.config",
            "python-config-cancel-test".into(),
        );
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
        let events = ScanEventSink::new(sender);
        run_scanner_jobs(vec![job, native_job], 2, events, PauseGate::default(), CancellationToken::new()).await;

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
                    ..
                } => saw_cancelled = true,
                _ => {}
            }
        }

        assert!(saw_python_discovery);
        assert!(saw_cancelled);
    }
    #[cfg(windows)]
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
        let events = ScanEventSink::new(sender);
        run_scanner_jobs(
            vec![python_job, native_job],
            2,
            events,
            PauseGate::default(),
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
                    ..
                } => saw_completed = true,
                _ => {}
            }
        }

        assert!(saw_python_failure);
        assert!(saw_completed);
    }
    #[tokio::test]
    async fn filesystem_job_finds_generic_signal_and_finishes() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("example-mcp.json"), "{}").unwrap();

        let roots = vec![root.path().to_path_buf()];
        let job = ScannerJob::new(
            "filesystem.quick",
            Duration::from_secs(5),
            move |events, cancellation| async move {
                match tokio::task::spawn_blocking(move || {
                    scan_blocking(roots, events, cancellation)
                })
                .await
                {
                    Ok(terminal) => terminal,
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "The filesystem scanner stopped unexpectedly".into(),
                    },
                }
            },
        );

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);
        let run = tokio::spawn(async move {
            run_scanner_jobs(vec![job], 1, events, PauseGate::default(), CancellationToken::new()).await;
        });

        let mut found = false;
        let mut completed = false;

        while let Some(event) = receiver.recv().await {
            match event {
                ScanEvent::Discovery { discovery }
                    if discovery.source_scanner == "filesystem.quick"
                        && discovery.suggested_name == "example-mcp.json" =>
                {
                    found = true;
                }
                ScanEvent::Completed { discovered, .. } => {
                    assert_eq!(discovered, 1);
                    completed = true;
                    break;
                }
                _ => {}
            }
        }

        run.await.unwrap();

        assert!(found);
        assert!(completed);
    }
    #[test]
    fn scanner_failed_events_preserve_scanner_id() {
        let event = ScanEvent::ScannerFailed {
            scanner_id: "filesystem.quick".into(),
            code: "access_denied".into(),
            message: "partial failure".into(),
        };

        let encoded = serde_json::to_value(event).unwrap();

        assert_eq!(
            encoded.get("scanner_id").and_then(|value| value.as_str()),
            Some("filesystem.quick")
        );
    }

    #[tokio::test]
    async fn scanner_panic_is_isolated_and_scan_still_settles() {
        use std::time::Duration;

        let jobs = vec![
            ScannerJob::new(
                "test.panic",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    panic!("scanner panic");
                },
            ),
            ScannerJob::new(
                "test.ok",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    ScannerTerminal::Completed {
                        visited: 2,
                        discovered: 1,
                    }
                },
            ),
        ];

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        run_scanner_jobs(jobs, 2, events, PauseGate::default(), CancellationToken::new()).await;

        let mut saw_failure = false;
        let mut saw_completed = false;

        while let Ok(event) = receiver.try_recv() {
            match event {
                ScanEvent::ScannerFailed {
                    scanner_id, code, ..
                } if scanner_id == "test.panic" && code == "scanner_failed" => {
                    saw_failure = true;
                }
                ScanEvent::Completed {
                    visited: 2,
                    discovered: 1,
                    ..
                } => {
                    saw_completed = true;
                }
                _ => {}
            }
        }

        assert!(saw_failure);
        assert!(saw_completed);
    }

    #[tokio::test]
    async fn scanner_results_are_settled_in_completion_order() {
        use std::time::Duration;

        let jobs = vec![
            ScannerJob::new(
                "test.slow",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    ScannerTerminal::Completed {
                        visited: 1,
                        discovered: 0,
                    }
                },
            ),
            ScannerJob::new(
                "test.fast",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    ScannerTerminal::Failed {
                        code: "fast_failure".into(),
                        message: "fast scanner failed".into(),
                    }
                },
            ),
        ];

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        let run = tokio::spawn(async move {
            run_scanner_jobs(jobs, 2, events, PauseGate::default(), CancellationToken::new()).await;
        });

        tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                let event = receiver
                    .recv()
                    .await
                    .expect("scanner event channel closed unexpectedly");

                if matches!(
                    event,
                    ScanEvent::ScannerFailed {
                        scanner_id,
                        code,
                        ..
                    } if scanner_id == "test.fast" && code == "fast_failure"
                ) {
                    break;
                }
            }
        })
        .await
        .expect("fast scanner settlement was delayed behind slow scanner");

        run.await.unwrap();
    }

    #[tokio::test]
    async fn root_cancellation_prevents_queued_scanners_from_starting() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let queued_started = Arc::new(AtomicUsize::new(0));
        let queued_started_for_job = queued_started.clone();

        let jobs = vec![
            ScannerJob::new(
                "test.blocking",
                Duration::from_secs(1),
                |_events, cancellation| async move {
                    cancellation.cancelled().await;
                    ScannerTerminal::Cancelled {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ),
            ScannerJob::new(
                "test.queued",
                Duration::from_secs(1),
                move |_events, _cancellation| async move {
                    queued_started_for_job.fetch_add(1, Ordering::SeqCst);
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ),
        ];

        let cancellation = CancellationToken::new();
        let run_cancellation = cancellation.clone();
        let (sender, _receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        let run = tokio::spawn(async move {
            run_scanner_jobs(jobs, 1, events, PauseGate::default(), run_cancellation).await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        cancellation.cancel();

        run.await.unwrap();

        assert_eq!(queued_started.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn scanner_timeout_is_isolated_from_other_jobs() {
        use std::time::Duration;

        let jobs = vec![
            ScannerJob::new(
                "test.slow",
                Duration::from_millis(10),
                |_events, _cancellation| async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    ScannerTerminal::Completed {
                        visited: 9,
                        discovered: 9,
                    }
                },
            ),
            ScannerJob::new(
                "test.fast",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    ScannerTerminal::Completed {
                        visited: 3,
                        discovered: 1,
                    }
                },
            ),
        ];

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        run_scanner_jobs(jobs, 2, events, PauseGate::default(), CancellationToken::new()).await;

        let mut saw_timeout = false;
        let mut saw_completed = false;

        while let Ok(event) = receiver.try_recv() {
            match event {
                ScanEvent::ScannerFailed {
                    scanner_id, code, ..
                } if scanner_id == "test.slow" && code == "scanner_timeout" => {
                    saw_timeout = true;
                }
                ScanEvent::Completed {
                    visited: 3,
                    discovered: 1,
                    ..
                } => {
                    saw_completed = true;
                }
                _ => {}
            }
        }

        assert!(saw_timeout);
        assert!(saw_completed);
    }

    #[tokio::test]
    async fn scanner_jobs_respect_bounded_concurrency() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut jobs = Vec::new();

        for index in 0..6 {
            let active = active.clone();
            let peak = peak.clone();

            jobs.push(ScannerJob::new(
                format!("test.{index}"),
                Duration::from_secs(1),
                move |_events, _cancellation| async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);

                    tokio::time::sleep(Duration::from_millis(25)).await;

                    active.fetch_sub(1, Ordering::SeqCst);
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ));
        }

        let (sender, _receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        run_scanner_jobs(jobs, 2, events, PauseGate::default(), CancellationToken::new()).await;

        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn quick_scan_concurrency_caps_simultaneous_scanner_jobs_at_four() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let mut jobs = Vec::new();

        for index in 0..6 {
            let active = active.clone();
            let max_seen = max_seen.clone();

            jobs.push(ScannerJob::new(
                format!("test.{index}"),
                Duration::from_secs(1),
                move |_events, _cancellation| async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(current, Ordering::SeqCst);

                    tokio::time::sleep(Duration::from_millis(25)).await;

                    active.fetch_sub(1, Ordering::SeqCst);
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ));
        }

        let (sender, _receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        run_scanner_jobs(
            jobs,
            QUICK_SCAN_CONCURRENCY,
            events,
            PauseGate::default(),
            CancellationToken::new(),
        )
        .await;

        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(QUICK_SCAN_CONCURRENCY, 4);
        assert_eq!(max_seen.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn pausing_before_release_blocks_queued_jobs_but_lets_running_jobs_finish() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let mut jobs = Vec::new();

        // With concurrency 1, job index 0 acquires the sole permit
        // immediately and is already running its bounded operation when
        // the gate is paused, so it must be allowed to finish. Jobs 1-4
        // are still queued behind the semaphore and must not enter their
        // runner (job "5" in the brief's numbering) until `resume()`.
        for index in 0..5 {
            let started = started.clone();
            let finished = finished.clone();

            jobs.push(ScannerJob::new(
                format!("test.{index}"),
                Duration::from_secs(5),
                move |_events, _cancellation| async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    finished.fetch_add(1, Ordering::SeqCst);
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ));
        }

        let pause_gate = PauseGate::default();
        let (sender, _receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        let run = tokio::spawn({
            let pause_gate = pause_gate.clone();
            async move {
                run_scanner_jobs(jobs, 1, events, pause_gate, CancellationToken::new()).await;
            }
        });

        // Let the first job acquire its permit and start, then pause
        // before any further queued job can enter its runner.
        tokio::time::sleep(Duration::from_millis(5)).await;
        pause_gate.pause();

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "only the already-running job may have started while paused"
        );
        assert_eq!(
            finished.load(Ordering::SeqCst),
            1,
            "the already-running job must be allowed to settle while paused"
        );

        pause_gate.resume();
        run.await.unwrap();

        assert_eq!(started.load(Ordering::SeqCst), 5);
        assert_eq!(finished.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn progress_events_preserve_scanner_identity() {
        let event = ScanEvent::Progress {
            scanner_id: "windows.process".into(),
            completed_units: 7,
            total_units: None,
            current_location: None,
        };

        assert!(matches!(
            event,
            ScanEvent::Progress {
                scanner_id,
                completed_units: 7,
                ..
            } if scanner_id == "windows.process"
        ));
    }

    #[tokio::test]
    async fn scanner_jobs_emit_progress_identity_when_they_start() {
        let jobs = vec![
            ScannerJob::new(
                "test.one",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ),
            ScannerJob::new(
                "test.two",
                Duration::from_secs(1),
                |_events, _cancellation| async move {
                    ScannerTerminal::Completed {
                        visited: 0,
                        discovered: 0,
                    }
                },
            ),
        ];

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        run_scanner_jobs(jobs, 2, events, PauseGate::default(), CancellationToken::new()).await;

        let mut scanner_ids = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            if let ScanEvent::Progress {
                scanner_id,
                completed_units: 0,
                ..
            } = event
            {
                scanner_ids.push(scanner_id);
            }
        }

        scanner_ids.sort();

        assert_eq!(
            scanner_ids,
            vec!["test.one".to_string(), "test.two".to_string()]
        );
    }

    #[tokio::test]
    async fn full_progress_channel_does_not_block_scanner_start() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let started = Arc::new(AtomicBool::new(false));
        let started_for_job = started.clone();

        let job = ScannerJob::new(
            "test.nonblocking-progress",
            Duration::from_secs(1),
            move |_events, _cancellation| async move {
                started_for_job.store(true, Ordering::SeqCst);
                ScannerTerminal::Completed {
                    visited: 0,
                    discovered: 0,
                }
            },
        );

        let (sender, mut receiver) = mpsc::channel(1);
        let events = ScanEventSink::new(sender);
        events.progress(ScanEvent::Progress {
            scanner_id: "test.prefill".into(),
            completed_units: 0,
            total_units: None,
            current_location: None,
        });

        let run = tokio::spawn(async move {
            run_scanner_jobs(vec![job], 1, events, PauseGate::default(), CancellationToken::new()).await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        assert!(
            started.load(Ordering::SeqCst),
            "scanner start was blocked by a full progress channel"
        );

        while let Some(event) = receiver.recv().await {
            if matches!(event, ScanEvent::Completed { .. }) {
                break;
            }
        }

        run.await.unwrap();
    }

    #[tokio::test]
    async fn scanner_failure_is_isolated_until_other_scanners_settle() {
        let mut coordinator = ScanCoordinatorState::new(
            ["windows.process", "windows.path"],
            CancellationToken::new(),
        );
        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);

        coordinator
            .settle(
                "windows.process",
                ScannerTerminal::Failed {
                    code: "access_denied".into(),
                    message: "partial failure".into(),
                },
                &events,
            )
            .await;

        assert!(matches!(
            receiver.try_recv(),
            Ok(ScanEvent::ScannerFailed {
                scanner_id,
                code,
                ..
            }) if scanner_id == "windows.process" && code == "access_denied"
        ));
        assert!(!coordinator.is_terminal());

        coordinator
            .settle(
                "windows.path",
                ScannerTerminal::Completed {
                    visited: 3,
                    discovered: 1,
                },
                &events,
            )
            .await;

        assert!(matches!(
            receiver.try_recv(),
            Ok(ScanEvent::Completed {
                visited: 3,
                discovered: 1,
                ..
            })
        ));
        assert!(coordinator.is_terminal());
    }

    #[test]
    fn scan_settlement_waits_for_every_selected_scanner() {
        let mut settlement =
            ScanSettlement::new(["windows.path", "windows.process", "windows.services"]);

        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.path");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.path");
        settlement.mark_settled("unknown");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.process");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.services");
        assert!(settlement.is_terminal());
    }

    #[tokio::test]
    async fn skips_high_noise_cache_directories() {
        let root = tempdir().unwrap();
        let cache = root.path().join("Cache");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("example-mcp.json"), "{}").unwrap();

        let roots = vec![root.path().to_path_buf()];
        let job = ScannerJob::new(
            "filesystem.quick",
            Duration::from_secs(5),
            move |events, cancellation| async move {
                match tokio::task::spawn_blocking(move || {
                    scan_blocking(roots, events, cancellation)
                })
                .await
                {
                    Ok(terminal) => terminal,
                    Err(_) => ScannerTerminal::Failed {
                        code: "scanner_failed".into(),
                        message: "The filesystem scanner stopped unexpectedly".into(),
                    },
                }
            },
        );

        let (sender, mut receiver) = mpsc::channel(16);
        let events = ScanEventSink::new(sender);
        let run = tokio::spawn(async move {
            run_scanner_jobs(vec![job], 1, events, PauseGate::default(), CancellationToken::new()).await;
        });

        let mut completed = false;

        while let Some(event) = receiver.recv().await {
            match event {
                ScanEvent::Discovery { discovery }
                    if discovery.source_scanner == "filesystem.quick" =>
                {
                    panic!("cache discovery must be excluded");
                }
                ScanEvent::Completed { discovered, .. } => {
                    assert_eq!(discovered, 0);
                    completed = true;
                    break;
                }
                _ => {}
            }
        }

        run.await.unwrap();

        assert!(completed);
    }
}
