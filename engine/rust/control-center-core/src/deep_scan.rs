//! Deep Scan traversal engine.
//!
//! Deep Scan runs a single scanner, `filesystem.deep`, over an explicit set
//! of roots the user selected. It does not rerun the eight Quick Scan
//! scanners. Traversal reads at most [`DEEP_DIRECTORY_CONCURRENCY`]
//! directories concurrently, cooperatively pauses before launching new
//! directory reads (an in-flight read may still settle while paused),
//! remains cancellable while paused, never follows symbolic links or
//! junctions unless the caller opts in, and — when it does follow them —
//! prevents cycles via the stable directory identity from
//! [`crate::deep_scan_windows`].

use crate::Discovery;
use crate::deep_scan_windows::{
    DirectoryIdentity, EntryPolicy, RootLocation, classify_root, entry_policy,
    stable_directory_identity,
};
use crate::scan::{fingerprint, looks_relevant, quick_excluded_names};
use crate::scan_control::{PauseGate, ScanEvent, ScanEventSink, ScanScope};
use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// Deep Scan reads at most this many directories concurrently.
pub const DEEP_DIRECTORY_CONCURRENCY: usize = 8;

const SCANNER_ID: &str = "filesystem.deep";

/// The roots and options a caller selected for a single Deep Scan run.
/// `network_consent` is per-request only: Deep Scan never persists it.
#[derive(Debug, Clone)]
pub struct DeepScanContext {
    pub roots: Vec<PathBuf>,
    pub follow_reparse_points: bool,
    pub network_consent: bool,
}

/// Errors that stop a Deep Scan run before (or instead of) traversal.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeepScanError {
    #[error("no roots were selected for deep scan")]
    NoRootsSelected,
    #[error("a selected root is on the network and requires explicit consent for this scan")]
    NetworkConsentRequired,
    #[error("a selected root could not be classified")]
    RootUnavailable,
}

impl DeepScanError {
    pub fn code(&self) -> &'static str {
        match self {
            DeepScanError::NoRootsSelected => "no_roots_selected",
            DeepScanError::NetworkConsentRequired => "network_consent_required",
            DeepScanError::RootUnavailable => "root_unavailable",
        }
    }
}

/// Validates the roots a Deep Scan is about to run over. Never persists
/// `network_consent`; it is checked fresh for this call only.
///
/// Exposed publicly so callers (the Tauri command layer) can run the same
/// check synchronously, before a scan is admitted, rather than only inside
/// the spawned `deep_scan` task.
pub fn validate_deep_roots(roots: &[PathBuf], network_consent: bool) -> Result<(), DeepScanError> {
    validate_roots(roots, network_consent)
}

fn validate_roots(roots: &[PathBuf], network_consent: bool) -> Result<(), DeepScanError> {
    if roots.is_empty() {
        return Err(DeepScanError::NoRootsSelected);
    }

    for root in roots {
        match classify_root(root) {
            Ok(RootLocation::Network) if !network_consent => {
                return Err(DeepScanError::NetworkConsentRequired);
            }
            Ok(_) => {}
            Err(error) => {
                // The underlying io::Error may embed the raw path (e.g.
                // "path has no drive or UNC prefix to classify: {path}").
                // Log it locally only; never let it reach a ScanEvent or
                // CommandError, which are serialized to the frontend and
                // may be persisted.
                eprintln!(
                    "deep scan root could not be classified: root={} error={error}",
                    root.display()
                );
                return Err(DeepScanError::RootUnavailable);
            }
        }
    }

    Ok(())
}

/// One directory-read unit of work in the traversal queue.
struct DirectoryWork {
    path: PathBuf,
    root_index: usize,
    depth: usize,
}

/// A single filesystem entry as reported by a [`DeepScanIo`] implementation.
struct DeepEntry {
    path: PathBuf,
    file_name: String,
    is_dir: bool,
}

/// Filesystem access seam for the traversal engine. `NativeDeepScanIo`
/// delegates to the real filesystem and the Task 4 safety helpers; tests
/// use an in-memory fake so concurrency, pause and cycle behavior are
/// deterministic and independent of disk timing.
trait DeepScanIo: Send + Sync + 'static {
    fn read_directory(&self, path: &Path) -> io::Result<Vec<DeepEntry>>;
    fn entry_policy(&self, path: &Path) -> io::Result<EntryPolicy>;
    fn directory_identity(&self, path: &Path) -> io::Result<DirectoryIdentity>;
}

struct NativeDeepScanIo;

impl DeepScanIo for NativeDeepScanIo {
    fn read_directory(&self, path: &Path) -> io::Result<Vec<DeepEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let is_dir = entry.file_type()?.is_dir();
            entries.push(DeepEntry {
                path: entry.path(),
                file_name: entry.file_name().to_string_lossy().into_owned(),
                is_dir,
            });
        }
        Ok(entries)
    }

    fn entry_policy(&self, path: &Path) -> io::Result<EntryPolicy> {
        entry_policy(path)
    }

    fn directory_identity(&self, path: &Path) -> io::Result<DirectoryIdentity> {
        stable_directory_identity(path)
    }
}

fn is_excluded(name: &str) -> bool {
    quick_excluded_names()
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
}

fn stable_error_code(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::NotFound => "entry_missing",
        _ => "directory_read_failed",
    }
}

/// Result of one directory read task, handed back through the `JoinSet`.
struct DirTaskResult {
    work: DirectoryWork,
    outcome: io::Result<Vec<DeepEntry>>,
}

/// Applies exclusion, placeholder, reparse-following and cycle-prevention
/// policy to one directory's entries. Returns the child directories to
/// queue and the discoveries to emit; never reads file content.
fn apply_policy(
    entries: Vec<DeepEntry>,
    work: &DirectoryWork,
    context: &DeepScanContext,
    io: &dyn DeepScanIo,
    identities_seen: &mut HashSet<DirectoryIdentity>,
    metadata_error_codes: &mut HashSet<&'static str>,
) -> (Vec<DirectoryWork>, Vec<Discovery>, Vec<&'static str>) {
    let mut children = Vec::new();
    let mut discoveries = Vec::new();
    let mut new_error_codes = Vec::new();

    for entry in entries {
        if is_excluded(&entry.file_name) {
            continue;
        }

        let policy = match io.entry_policy(&entry.path) {
            Ok(policy) => policy,
            Err(_) => {
                if metadata_error_codes.insert("entry_metadata_unavailable") {
                    new_error_codes.push("entry_metadata_unavailable");
                }
                continue;
            }
        };

        if policy.placeholder {
            continue;
        }

        if entry.is_dir {
            if policy.reparse_point && !context.follow_reparse_points {
                continue;
            }

            if context.follow_reparse_points {
                let identity = match io.directory_identity(&entry.path) {
                    Ok(identity) => identity,
                    Err(_) => {
                        if metadata_error_codes.insert("entry_metadata_unavailable") {
                            new_error_codes.push("entry_metadata_unavailable");
                        }
                        continue;
                    }
                };
                if !identities_seen.insert(identity) {
                    // Already visited this directory under a different
                    // path (hard link, junction cycle, ...): skip.
                    continue;
                }
            }

            children.push(DirectoryWork {
                path: entry.path,
                root_index: work.root_index,
                depth: work.depth + 1,
            });
        } else if looks_relevant(&entry.path) {
            discoveries.push(Discovery::unknown(
                entry.file_name.clone(),
                SCANNER_ID,
                fingerprint(&entry.path),
            ));
        }
    }

    (children, discoveries, new_error_codes)
}

/// Runs Deep Scan traversal against a real filesystem.
pub async fn deep_scan(
    context: DeepScanContext,
    events: ScanEventSink,
    cancellation: CancellationToken,
    pause_gate: PauseGate,
) {
    run_deep_scan(
        context,
        events,
        cancellation,
        pause_gate,
        Arc::new(NativeDeepScanIo),
    )
    .await
}

async fn run_deep_scan(
    context: DeepScanContext,
    events: ScanEventSink,
    cancellation: CancellationToken,
    pause_gate: PauseGate,
    io: Arc<dyn DeepScanIo>,
) {
    let start = Instant::now();

    if let Err(error) = validate_roots(&context.roots, context.network_consent) {
        // `error.to_string()` is path-free by construction (see
        // `DeepScanError`'s Display impls): raw paths are logged locally at
        // the point they are observed, never carried into a wire event.
        let _ = events
            .critical(ScanEvent::Failed {
                code: error.code().into(),
                message: error.to_string(),
                failure_count: events.failure_count(),
                duration_ms: start.elapsed().as_millis() as u64,
            })
            .await;
        return;
    }

    if events
        .critical(ScanEvent::Started {
            scope: ScanScope::Deep,
            scanner_count: 1,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut join_set: JoinSet<DirTaskResult> = JoinSet::new();
    let mut identities_seen: HashSet<DirectoryIdentity> = HashSet::new();
    let mut reported_error_codes: HashSet<&'static str> = HashSet::new();
    let mut metadata_error_codes: HashSet<&'static str> = HashSet::new();
    let mut visited: u64 = 0;
    let mut discovered: u64 = 0;
    let mut stop_launching = false;
    let mut coordinator_failed = false;

    // Explicitly selected roots get the same safety policy as every entry
    // discovered during traversal -- validate_roots above only classifies
    // the root's own selected path (UNC prefix / drive type) and cannot see
    // that the root itself is a placeholder, or a reparse point/junction
    // whose target differs from what its own path implies.
    let mut seed_roots = Vec::with_capacity(context.roots.len());
    for (root_index, root) in context.roots.iter().enumerate() {
        match io.entry_policy(root) {
            Ok(policy) => {
                if policy.placeholder {
                    // Same as any placeholder entry: never traversed, never
                    // hydrated.
                    continue;
                }
                if policy.reparse_point {
                    if !context.follow_reparse_points {
                        // Default refusal applies to roots exactly as it
                        // does to any discovered entry.
                        continue;
                    }
                    if !context.network_consent {
                        // A local reparse point's target cannot be
                        // classified as local-vs-network from the selected
                        // path's own syntax (classify_root only inspects
                        // that path, not what it resolves to). Require
                        // consent for any followed root reparse point,
                        // exactly as for a directly-selected network root,
                        // rather than trusting the root's syntactic
                        // classification -- closes the bypass where a local
                        // junction/symlink resolves to network storage.
                        let _ = events
                            .critical(ScanEvent::Failed {
                                code: DeepScanError::NetworkConsentRequired.code().into(),
                                message: DeepScanError::NetworkConsentRequired.to_string(),
                                failure_count: events.failure_count(),
                                duration_ms: start.elapsed().as_millis() as u64,
                            })
                            .await;
                        return;
                    }
                }
            }
            Err(_) => {
                if metadata_error_codes.insert("entry_metadata_unavailable") {
                    let _ = events
                        .scanner_failed(
                            SCANNER_ID,
                            "entry_metadata_unavailable",
                            "A selected root's metadata could not be read",
                        )
                        .await;
                }
                continue;
            }
        }
        seed_roots.push((root_index, root.clone()));
    }

    let mut queue: VecDeque<DirectoryWork> = seed_roots
        .into_iter()
        .map(|(root_index, root)| DirectoryWork {
            path: root,
            root_index,
            depth: 0,
        })
        .collect();

    loop {
        if !stop_launching {
            while join_set.len() < DEEP_DIRECTORY_CONCURRENCY {
                if !pause_gate.checkpoint(&cancellation).await {
                    stop_launching = true;
                    break;
                }

                let Some(work) = queue.pop_front() else {
                    break;
                };

                let task_io = io.clone();
                join_set.spawn_blocking(move || {
                    let outcome = task_io.read_directory(&work.path);
                    DirTaskResult { work, outcome }
                });
            }
        }

        if join_set.is_empty() {
            break;
        }

        let joined = match join_set.join_next().await {
            Some(joined) => joined,
            None => break,
        };

        let DirTaskResult { work, outcome } = match joined {
            Ok(result) => result,
            Err(_) => {
                // A directory-read task panicked or was aborted. Isolate the
                // failure to this one directory (mirroring how Quick Scan
                // isolates a single scanner panic) rather than aborting the
                // entire traversal and discarding everything found so far.
                visited += 1;
                let code = "directory_read_panicked";
                if reported_error_codes.insert(code) {
                    let _ = events
                        .scanner_failed(
                            SCANNER_ID,
                            code,
                            format!("Skipped a directory that could not be read ({code})"),
                        )
                        .await;
                }
                continue;
            }
        };

        visited += 1;
        events.progress(ScanEvent::Progress {
            scanner_id: SCANNER_ID.into(),
            completed_units: visited,
            total_units: None,
            current_location: Some(format!(
                "Selected root {} · depth {}",
                work.root_index + 1,
                work.depth
            )),
        });

        match outcome {
            Ok(entries) => {
                let (children, discoveries, new_metadata_error_codes) = apply_policy(
                    entries,
                    &work,
                    &context,
                    io.as_ref(),
                    &mut identities_seen,
                    &mut metadata_error_codes,
                );

                for code in new_metadata_error_codes {
                    if events
                        .scanner_failed(
                            SCANNER_ID,
                            code,
                            format!("Skipped an entry whose metadata could not be read ({code})"),
                        )
                        .await
                        .is_err()
                    {
                        coordinator_failed = true;
                        break;
                    }
                }

                if coordinator_failed {
                    break;
                }

                for child in children {
                    queue.push_back(child);
                }

                for discovery in discoveries {
                    if events
                        .critical(ScanEvent::Discovery { discovery })
                        .await
                        .is_err()
                    {
                        coordinator_failed = true;
                        break;
                    }
                    discovered += 1;
                }

                if coordinator_failed {
                    break;
                }
            }
            Err(error) => {
                let code = stable_error_code(&error);
                if reported_error_codes.insert(code) {
                    let _ = events
                        .scanner_failed(
                            SCANNER_ID,
                            code,
                            format!("Skipped a directory that could not be read ({code})"),
                        )
                        .await;
                }
            }
        }
    }

    let failure_count = events.failure_count();
    let duration_ms = start.elapsed().as_millis() as u64;

    if coordinator_failed {
        let _ = events
            .critical(ScanEvent::Failed {
                code: "deep_scan_coordinator_failed".into(),
                message: "Deep scan traversal stopped due to an internal coordinator error".into(),
                failure_count,
                duration_ms,
            })
            .await;
        return;
    }

    let cancelled = cancellation.is_cancelled();
    let terminal = if cancelled {
        ScanEvent::Cancelled {
            visited,
            discovered,
            failure_count,
            duration_ms,
        }
    } else {
        ScanEvent::Completed {
            visited,
            discovered,
            failure_count,
            duration_ms,
        }
    };

    let _ = events.critical(terminal).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[test]
    fn network_root_without_consent_is_rejected() {
        let unc_root = PathBuf::from(r"\\server\share\folder");
        assert!(matches!(
            validate_roots(std::slice::from_ref(&unc_root), false),
            Err(DeepScanError::NetworkConsentRequired)
        ));
        assert!(validate_roots(&[unc_root], true).is_ok());
    }

    #[test]
    fn empty_root_list_is_rejected() {
        assert!(matches!(
            validate_roots(&[], false),
            Err(DeepScanError::NoRootsSelected)
        ));
    }

    #[test]
    fn root_unavailable_error_and_its_wire_message_never_carry_the_raw_path() {
        // A path with no drive letter and no UNC prefix cannot be classified
        // and deterministically triggers `DeepScanError::RootUnavailable`.
        let secret_root = PathBuf::from("relative/unclassifiable/root-marker-72f1");

        let error = validate_roots(std::slice::from_ref(&secret_root), false).unwrap_err();
        assert!(matches!(error, DeepScanError::RootUnavailable));

        // The Display impl (what ends up in ScanEvent::Failed.message and in
        // CommandError::invalid_request(...)) must never contain the raw path.
        let wire_message = error.to_string();
        assert!(
            !wire_message.contains("root-marker-72f1"),
            "RootUnavailable's wire message leaked the raw path: {wire_message:?}"
        );
        assert!(
            !wire_message.contains(secret_root.to_string_lossy().as_ref()),
            "RootUnavailable's wire message leaked the raw path: {wire_message:?}"
        );
    }

    #[derive(Clone)]
    struct FakeEntry {
        name: String,
        is_dir: bool,
        reparse: bool,
        placeholder: bool,
        identity: Option<DirectoryIdentity>,
        metadata_error: bool,
    }

    fn dir(name: &str) -> FakeEntry {
        FakeEntry {
            name: name.into(),
            is_dir: true,
            reparse: false,
            placeholder: false,
            identity: None,
            metadata_error: false,
        }
    }

    fn file(name: &str) -> FakeEntry {
        FakeEntry {
            name: name.into(),
            is_dir: false,
            reparse: false,
            placeholder: false,
            identity: None,
            metadata_error: false,
        }
    }

    struct FakeDeepScanIo {
        tree: HashMap<PathBuf, Vec<FakeEntry>>,
        active_reads: AtomicUsize,
        max_active_reads: AtomicUsize,
        read_starts: AtomicUsize,
        read_log: Mutex<Vec<PathBuf>>,
        panic_on_read: Option<PathBuf>,
    }

    impl FakeDeepScanIo {
        fn new(tree: HashMap<PathBuf, Vec<FakeEntry>>) -> Self {
            Self {
                tree,
                active_reads: AtomicUsize::new(0),
                max_active_reads: AtomicUsize::new(0),
                read_starts: AtomicUsize::new(0),
                read_log: Mutex::new(Vec::new()),
                panic_on_read: None,
            }
        }

        fn with_panic_on_read(tree: HashMap<PathBuf, Vec<FakeEntry>>, panic_path: PathBuf) -> Self {
            Self {
                tree,
                active_reads: AtomicUsize::new(0),
                max_active_reads: AtomicUsize::new(0),
                read_starts: AtomicUsize::new(0),
                read_log: Mutex::new(Vec::new()),
                panic_on_read: Some(panic_path),
            }
        }

        fn max_active_reads(&self) -> usize {
            self.max_active_reads.load(Ordering::SeqCst)
        }

        fn active_reads(&self) -> usize {
            self.active_reads.load(Ordering::SeqCst)
        }

        fn read_starts(&self) -> usize {
            self.read_starts.load(Ordering::SeqCst)
        }

        fn was_read(&self, path: &Path) -> bool {
            self.read_log
                .lock()
                .unwrap()
                .iter()
                .any(|logged| logged == path)
        }

        fn find(&self, parent: &Path, name: &str) -> Option<FakeEntry> {
            self.tree
                .get(parent)
                .and_then(|entries| entries.iter().find(|entry| entry.name == name))
                .cloned()
        }
    }

    impl DeepScanIo for FakeDeepScanIo {
        fn read_directory(&self, path: &Path) -> io::Result<Vec<DeepEntry>> {
            self.read_starts.fetch_add(1, Ordering::SeqCst);
            self.read_log.lock().unwrap().push(path.to_path_buf());
            let current = self.active_reads.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active_reads.fetch_max(current, Ordering::SeqCst);

            std::thread::sleep(Duration::from_millis(25));

            self.active_reads.fetch_sub(1, Ordering::SeqCst);

            if self.panic_on_read.as_deref() == Some(path) {
                panic!("simulated directory read panic");
            }

            let entries = self.tree.get(path).cloned().unwrap_or_default();
            Ok(entries
                .into_iter()
                .map(|entry| DeepEntry {
                    path: path.join(&entry.name),
                    file_name: entry.name,
                    is_dir: entry.is_dir,
                })
                .collect())
        }

        fn entry_policy(&self, path: &Path) -> io::Result<EntryPolicy> {
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            match self.find(parent, name) {
                Some(entry) if entry.metadata_error => {
                    Err(io::Error::other("simulated metadata error"))
                }
                Some(entry) => Ok(EntryPolicy {
                    reparse_point: entry.reparse,
                    placeholder: entry.placeholder,
                }),
                None => Ok(EntryPolicy::default()),
            }
        }

        fn directory_identity(&self, path: &Path) -> io::Result<DirectoryIdentity> {
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            self.find(parent, name)
                .and_then(|entry| entry.identity)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no fake identity"))
        }
    }

    fn drain_events(mut rx: mpsc::Receiver<ScanEvent>) -> tokio::task::JoinHandle<Vec<ScanEvent>> {
        tokio::spawn(async move {
            let mut received = Vec::new();
            while let Some(event) = rx.recv().await {
                received.push(event);
            }
            received
        })
    }

    #[tokio::test]
    async fn concurrency_never_exceeds_eight() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut root_entries = Vec::new();
        let mut tree = HashMap::new();
        for i in 0..12 {
            let name = format!("dir{i}");
            root_entries.push(dir(&name));
            tree.insert(root.join(&name), Vec::new());
        }
        tree.insert(root.clone(), root_entries);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(512);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        assert_eq!(DEEP_DIRECTORY_CONCURRENCY, 8);
        assert_eq!(fake.max_active_reads(), 8);
    }

    #[tokio::test]
    async fn pause_blocks_next_directory_read_but_lets_in_flight_settle() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut root_entries = Vec::new();
        let mut tree = HashMap::new();
        for i in 0..12 {
            let name = format!("dir{i}");
            root_entries.push(dir(&name));
            tree.insert(root.join(&name), Vec::new());
        }
        tree.insert(root.clone(), root_entries);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(512);
        let events = ScanEventSink::new(tx);
        let pause_gate = PauseGate::default();
        let cancellation = CancellationToken::new();
        let drain = drain_events(rx);

        let scan_task = tokio::spawn(run_deep_scan(
            context,
            events,
            cancellation,
            pause_gate.clone(),
            fake.clone(),
        ));

        // Let the root read settle and the first batch of child reads
        // begin (root read = 25ms, plus scheduling slack).
        tokio::time::sleep(Duration::from_millis(60)).await;
        pause_gate.pause();
        let starts_at_pause = fake.read_starts();

        // Long enough for any in-flight batch to settle, but the paused
        // gate must prevent a *new* directory read from starting.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fake.active_reads(), 0, "in-flight reads must settle");
        assert_eq!(
            fake.read_starts(),
            starts_at_pause,
            "no new directory read may start while paused"
        );

        pause_gate.resume();
        tokio::time::timeout(Duration::from_secs(5), scan_task)
            .await
            .expect("scan must finish after resume")
            .expect("scan task must not panic");
        drop(drain);

        assert_eq!(
            fake.read_starts(),
            13,
            "root + 12 children must all be read"
        );
    }

    #[tokio::test]
    async fn cancel_while_paused_reaches_cancelled() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![dir("child")]);
        tree.insert(root.join("child"), Vec::new());

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, mut rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let pause_gate = PauseGate::default();
        let cancellation = CancellationToken::new();

        // Pause before the scan ever starts, so the very first checkpoint
        // blocks deterministically: nothing can be launched.
        pause_gate.pause();

        let scan_task = tokio::spawn(run_deep_scan(
            context,
            events,
            cancellation.clone(),
            pause_gate,
            fake.clone(),
        ));

        let started = rx.recv().await.expect("expected Started event");
        assert!(matches!(
            started,
            ScanEvent::Started {
                scope: ScanScope::Deep,
                scanner_count: 1,
            }
        ));

        cancellation.cancel();

        let terminal = rx.recv().await.expect("expected a terminal event");
        assert!(matches!(terminal, ScanEvent::Cancelled { .. }));
        assert_eq!(
            fake.read_starts(),
            0,
            "cancellation while paused must launch nothing"
        );

        scan_task.await.unwrap();
    }

    #[tokio::test]
    async fn excluded_directory_name_is_never_read() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![dir("node_modules")]);
        tree.insert(root.join("node_modules"), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        assert!(!fake.was_read(&root.join("node_modules")));
    }

    #[tokio::test]
    async fn reparse_directory_is_not_queued_when_following_is_disabled() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut linked = dir("linked");
        linked.reparse = true;
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![linked]);
        tree.insert(root.join("linked"), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        assert!(!fake.was_read(&root.join("linked")));
    }

    #[tokio::test]
    async fn reparse_root_is_not_traversed_when_following_is_disabled() {
        // The selected root itself (not a child entry) is a reparse point.
        // Regression for: run_deep_scan previously queued roots directly,
        // never calling entry_policy on them, so a reparse-point root
        // bypassed the default reparse refusal entirely.
        let root = PathBuf::from(r"C:\FakeRoot");
        let parent = PathBuf::from(r"C:\");
        let mut root_entry = dir("FakeRoot");
        root_entry.reparse = true;
        let mut tree = HashMap::new();
        tree.insert(parent, vec![root_entry]);
        tree.insert(root.clone(), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        assert!(
            !fake.was_read(&root),
            "a reparse-point root must not be traversed when follow_reparse_points is false"
        );
    }

    #[tokio::test]
    async fn placeholder_root_is_not_traversed() {
        // The selected root itself is a cloud-only/offline placeholder.
        let root = PathBuf::from(r"C:\FakeRoot");
        let parent = PathBuf::from(r"C:\");
        let mut root_entry = dir("FakeRoot");
        root_entry.placeholder = true;
        let mut tree = HashMap::new();
        tree.insert(parent, vec![root_entry]);
        tree.insert(root.clone(), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        assert!(
            !fake.was_read(&root),
            "a placeholder root must never be traversed or hydrated"
        );
    }

    #[tokio::test]
    async fn reparse_root_followed_without_network_consent_is_rejected() {
        // The selected root itself is a reparse point, and the caller
        // enabled follow_reparse_points but did not grant network consent.
        // classify_root would see this root's own local path (C:\...) and
        // classify it Local, but its reparse target is unknown -- it could
        // resolve to network storage. Regression for the consent bypass:
        // the scan must require consent for this case rather than trusting
        // the root's syntactic local classification.
        let root = PathBuf::from(r"C:\FakeRoot");
        let parent = PathBuf::from(r"C:\");
        let mut root_entry = dir("FakeRoot");
        root_entry.reparse = true;
        let mut tree = HashMap::new();
        tree.insert(parent, vec![root_entry]);
        tree.insert(root.clone(), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: true,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        assert!(
            !fake.was_read(&root),
            "a followed reparse root must not be traversed before network consent is confirmed"
        );
        assert!(
            received.iter().any(|event| matches!(
                event,
                ScanEvent::Failed { code, .. } if code == "network_consent_required"
            )),
            "expected a network_consent_required Failed event, got: {received:?}"
        );
    }

    #[tokio::test]
    async fn placeholder_file_is_skipped_and_never_becomes_a_discovery() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut placeholder = file("mcp-config.json");
        placeholder.placeholder = true;
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![placeholder]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        assert!(
            !received
                .iter()
                .any(|event| matches!(event, ScanEvent::Discovery { .. }))
        );
    }

    #[tokio::test]
    async fn same_directory_identity_is_read_only_once_when_following() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut a = dir("a");
        a.identity = Some(DirectoryIdentity::for_test("shared"));
        let mut b = dir("b");
        b.identity = Some(DirectoryIdentity::for_test("shared"));

        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![a, b]);
        tree.insert(root.join("a"), Vec::new());
        tree.insert(root.join("b"), Vec::new());

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: true,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        drop(drain);

        let read_a = fake.was_read(&root.join("a"));
        let read_b = fake.was_read(&root.join("b"));
        assert!(
            read_a ^ read_b,
            "exactly one of the two same-identity directories should be read"
        );
    }

    #[tokio::test]
    async fn cyclic_fake_graph_terminates() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut a = dir("a");
        a.identity = Some(DirectoryIdentity::for_test("a-identity"));
        let mut loop_back = dir("loop");
        loop_back.reparse = true;
        loop_back.identity = Some(DirectoryIdentity::for_test("a-identity"));

        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![a]);
        tree.insert(root.join("a"), vec![loop_back]);
        tree.insert(root.join("a").join("loop"), vec![dir("a")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: true,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            run_deep_scan(
                context,
                events,
                CancellationToken::new(),
                PauseGate::default(),
                fake.clone(),
            ),
        )
        .await;
        drop(drain);

        assert!(outcome.is_ok(), "cyclic graph must not hang traversal");
        assert!(!fake.was_read(&root_join_loop()));

        fn root_join_loop() -> PathBuf {
            PathBuf::from(r"C:\FakeRoot\a\loop\a")
        }
    }

    #[tokio::test]
    async fn progress_location_is_redacted() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![dir("a")]);
        tree.insert(root.join("a"), vec![dir("b")]);
        tree.insert(root.join("a").join("b"), Vec::new());

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        let depth_two = received.iter().find_map(|event| match event {
            ScanEvent::Progress {
                current_location: Some(location),
                ..
            } if location.ends_with("depth 2") => Some(location.clone()),
            _ => None,
        });

        let location = depth_two.expect("expected a depth-2 progress event");
        assert_eq!(location, "Selected root 1 · depth 2");
        assert!(!location.to_ascii_lowercase().contains("fakeroot"));
        assert!(!location.contains('\\'));
    }

    #[tokio::test]
    async fn discoveries_use_deep_source_and_started_is_first() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![file("mcp-config.json")]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        assert!(matches!(
            received.first(),
            Some(ScanEvent::Started {
                scope: ScanScope::Deep,
                scanner_count: 1,
            })
        ));

        let discovery = received
            .iter()
            .find_map(|event| match event {
                ScanEvent::Discovery { discovery } => Some(discovery),
                _ => None,
            })
            .expect("expected a discovery");
        assert_eq!(discovery.source_scanner, "filesystem.deep");
    }

    #[tokio::test]
    async fn entry_metadata_errors_surface_one_scanner_failed_event_per_distinct_code() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut bad_one = file("bad-one.txt");
        bad_one.metadata_error = true;
        let mut bad_two = file("bad-two.txt");
        bad_two.metadata_error = true;

        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![bad_one, bad_two]);

        let fake = Arc::new(FakeDeepScanIo::new(tree));
        let context = DeepScanContext {
            roots: vec![root],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        let failures: Vec<_> = received
            .iter()
            .filter_map(|event| match event {
                ScanEvent::ScannerFailed {
                    scanner_id,
                    code,
                    message,
                } => Some((scanner_id.clone(), code.clone(), message.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(
            failures.len(),
            1,
            "two entries with the same stable error code must emit only one event, got {failures:?}"
        );
        let (scanner_id, code, message) = &failures[0];
        assert_eq!(scanner_id, "filesystem.deep");
        assert_eq!(code, "entry_metadata_unavailable");
        assert!(!message.to_ascii_lowercase().contains("fakeroot"));
        assert!(!message.contains('\\'));

        let terminal = received.last().expect("expected a terminal event");
        match terminal {
            ScanEvent::Completed { failure_count, .. } => {
                assert!(
                    *failure_count >= 1,
                    "failure_count must reflect the metadata error"
                );
            }
            other => panic!("expected Completed terminal event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn directory_read_panic_is_isolated_and_scan_still_completes() {
        let root = PathBuf::from(r"C:\FakeRoot");
        let mut tree = HashMap::new();
        tree.insert(root.clone(), vec![dir("panics"), dir("ok")]);
        tree.insert(root.join("panics"), Vec::new());
        tree.insert(root.join("ok"), Vec::new());

        let fake = Arc::new(FakeDeepScanIo::with_panic_on_read(
            tree,
            root.join("panics"),
        ));
        let context = DeepScanContext {
            roots: vec![root.clone()],
            follow_reparse_points: false,
            network_consent: false,
        };
        let (tx, rx) = mpsc::channel(64);
        let events = ScanEventSink::new(tx);
        let drain = drain_events(rx);

        run_deep_scan(
            context,
            events,
            CancellationToken::new(),
            PauseGate::default(),
            fake.clone(),
        )
        .await;
        let received = drain.await.unwrap();

        assert!(
            fake.was_read(&root.join("ok")),
            "traversal must continue past the panicking directory"
        );

        let saw_failure = received.iter().any(|event| {
            matches!(
                event,
                ScanEvent::ScannerFailed { scanner_id, code, .. }
                    if scanner_id == "filesystem.deep" && code == "directory_read_panicked"
            )
        });
        assert!(
            saw_failure,
            "expected a ScannerFailed event for the panicking directory"
        );

        let terminal = received.last().expect("expected a terminal event");
        match terminal {
            ScanEvent::Completed { failure_count, .. } => {
                assert!(*failure_count >= 1);
            }
            other => panic!(
                "expected Completed terminal event (panic must not fail the whole scan), got {other:?}"
            ),
        }
    }
}
