use crate::Discovery;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

/// Which scan mode is running. Quick Scan is bounded and fast; Deep Scan is
/// exhaustive and long-running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanScope {
    Quick,
    Deep,
}

/// Lifecycle state shared by both scan modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanLifecycleState {
    Running,
    Paused,
    Cancelled,
    Completed,
    Failed,
}

/// Quick Scan runs at most this many scanner jobs concurrently.
pub const QUICK_SCAN_CONCURRENCY: usize = 4;

/// A cooperative pause gate. Pausing does not interrupt a bounded operation
/// that is already running; it only blocks new checkpointed work from
/// starting until `resume()` is called. Cancellation always takes
/// precedence over a pause.
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
    /// Pauses the gate. Returns `true` if this call transitioned the gate
    /// from running to paused, `false` if it was already paused.
    pub fn pause(&self) -> bool {
        !self.inner.paused.swap(true, Ordering::SeqCst)
    }

    /// Resumes the gate. Returns `true` if this call transitioned the gate
    /// from paused to running, `false` if it was already running.
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

    /// Blocks while the gate is paused. Returns `true` if the caller may
    /// proceed, `false` if `cancellation` fired first.
    pub async fn checkpoint(&self, cancellation: &CancellationToken) -> bool {
        self.checkpoint_inner(cancellation, || std::future::ready(()))
            .await
    }

    /// Core `checkpoint` logic, parameterized over a hook invoked exactly
    /// once per loop iteration, immediately after the `Notified` future is
    /// registered and immediately before `is_paused()` is read. Production
    /// code always calls this via `checkpoint()` with a no-op hook that
    /// resolves without yielding; the test module below uses an async hook
    /// to deterministically drive a concurrent `resume()` into that exact
    /// window, reproducing the tightest possible interleaving between the
    /// two calls without relying on real scheduling luck or sleeps.
    ///
    /// `notified()` MUST be called before `is_paused()` is read (not after,
    /// as an earlier version of this method did). `Notify::notify_waiters`
    /// only wakes tasks that registered by calling `.notified()` before
    /// `notify_waiters()` runs -- unlike `notify_one`, it stores no permit
    /// for a call that arrives late. Checking `is_paused()` first and
    /// registering `.notified()` second leaves a window: if `resume()` (and
    /// its `notify_waiters()`) lands in that window, the registration that
    /// follows misses it entirely, and `checkpoint` blocks on
    /// `notified().await` waiting for a call that already happened and may
    /// never happen again. Registering first closes the window: any
    /// `resume()` from the moment of registration onward is guaranteed to
    /// wake this waiter, and if `resume()` already happened before
    /// registration, `is_paused()` (read immediately after) already
    /// observes `false` and returns without waiting at all.
    async fn checkpoint_inner<F>(
        &self,
        cancellation: &CancellationToken,
        mut at_registration: impl FnMut() -> F,
    ) -> bool
    where
        F: std::future::Future<Output = ()>,
    {
        loop {
            if cancellation.is_cancelled() {
                return false;
            }

            let notified = self.inner.resumed.notified();
            at_registration().await;

            if !self.is_paused() {
                return true;
            }

            tokio::select! {
                _ = cancellation.cancelled() => return false,
                _ = notified => {}
            }
        }
    }

    /// Test-only entry point exposing the registration-window hook. See
    /// `checkpoint_inner`'s docs.
    #[cfg(test)]
    pub(crate) async fn checkpoint_with_hook<F>(
        &self,
        cancellation: &CancellationToken,
        hook: impl FnMut() -> F,
    ) -> bool
    where
        F: std::future::Future<Output = ()>,
    {
        self.checkpoint_inner(cancellation, hook).await
    }
}

/// Wire event emitted by a running scan. `kind` (snake_case) tags the
/// variant for the frontend.
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

/// Bounded sink for scan events. `Progress` events are best-effort: when the
/// channel is full, the newest one for a given `scanner_id` replaces (not
/// discards) whatever was already coalesced for that scanner in a small
/// side slot, bounded by the number of distinct scanner ids (at most 8 for
/// Quick, 1 for Deep) rather than by scan duration. Every other event class
/// remains lossless (awaited `send`), and always flushes any coalesced
/// progress first -- so the frontend eventually receives a current progress
/// state even if the exact tick that would have carried it was dropped, and
/// no progress is ever stranded behind a terminal event.
#[derive(Clone)]
pub struct ScanEventSink {
    tx: mpsc::Sender<ScanEvent>,
    failures: Arc<AtomicU64>,
    /// Latest dropped `Progress` event per `scanner_id`, pending flush.
    coalesced_progress: Arc<Mutex<HashMap<String, ScanEvent>>>,
}

impl ScanEventSink {
    pub fn new(tx: mpsc::Sender<ScanEvent>) -> Self {
        Self {
            tx,
            failures: Arc::new(AtomicU64::new(0)),
            coalesced_progress: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Best-effort send for high-frequency progress events. Never blocks.
    /// When the channel is full, the event is not discarded: it replaces
    /// any previously-coalesced-but-undelivered progress for the same
    /// `scanner_id`, to be flushed by the next `critical`/`blocking_critical`
    /// call (see their docs).
    pub fn progress(&self, event: ScanEvent) {
        debug_assert!(matches!(event, ScanEvent::Progress { .. }));
        if let Err(error) = self.tx.try_send(event)
            && let mpsc::error::TrySendError::Full(ScanEvent::Progress { scanner_id, .. }) = &error
        {
            let scanner_id = scanner_id.clone();
            let dropped = error.into_inner();
            self.coalesced_progress
                .lock()
                .unwrap()
                .insert(scanner_id, dropped);
        }
        // A `Closed` error means the receiver is gone (scan bridge already
        // torn down); there is nothing left to coalesce for.
    }

    /// Sends every coalesced-but-undelivered progress event before `event`
    /// itself, so a current progress state is never stranded behind a
    /// lossless event -- most importantly, never stranded behind terminal
    /// settlement.
    async fn flush_coalesced_progress_then(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        let pending: Vec<ScanEvent> = {
            let mut coalesced = self.coalesced_progress.lock().unwrap();
            coalesced.drain().map(|(_, event)| event).collect()
        };
        for progress in pending {
            self.tx.send(progress).await?;
        }
        self.tx.send(event).await
    }

    /// Lossless send for discoveries, lifecycle changes and terminal
    /// events. Awaits channel capacity rather than dropping. Flushes any
    /// coalesced progress first (see `flush_coalesced_progress_then`).
    pub async fn critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        self.flush_coalesced_progress_then(event).await
    }

    /// Lossless send for use from a blocking (non-async) context, such as
    /// a `spawn_blocking` filesystem walk. Blocks the current thread until
    /// channel capacity is available rather than dropping the event.
    ///
    /// The error is boxed (unlike the async siblings above, which are not
    /// flagged by `clippy::result_large_err` since it does not fire on
    /// `async fn`) because `SendError<ScanEvent>` embeds a whole `ScanEvent`
    /// and is large enough to make every caller's `Result` oversized even
    /// when the call succeeds.
    pub fn blocking_critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), Box<mpsc::error::SendError<ScanEvent>>> {
        let pending: Vec<ScanEvent> = {
            let mut coalesced = self.coalesced_progress.lock().unwrap();
            coalesced.drain().map(|(_, event)| event).collect()
        };
        for progress in pending {
            self.tx.blocking_send(progress).map_err(Box::new)?;
        }
        self.tx.blocking_send(event).map_err(Box::new)
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

    /// Deterministically reproduces the tightest possible interleaving for
    /// the PauseGate lost-wakeup race: a `resume()` (and its
    /// `notify_waiters()`) completing exactly between `checkpoint`'s
    /// `Notified` registration and its `is_paused()` read. Synchronized via
    /// oneshot channels, not sleeps -- the resumer task cannot proceed until
    /// the checkpoint task has registered, and the checkpoint task cannot
    /// proceed past the hook until the resumer has finished calling
    /// `resume()`, so this ordering is guaranteed on every run.
    #[tokio::test]
    async fn checkpoint_does_not_miss_a_resume_landing_between_registration_and_the_pause_check() {
        let gate = PauseGate::default();
        let cancellation = CancellationToken::new();
        assert!(gate.pause());

        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel::<()>();
        let (resumed_tx, resumed_rx) = tokio::sync::oneshot::channel::<()>();

        let resumer_gate = gate.clone();
        let resumer = tokio::spawn(async move {
            registered_rx.await.unwrap();
            assert!(resumer_gate.resume());
            let _ = resumed_tx.send(());
        });

        let mut registered_tx = Some(registered_tx);
        let mut resumed_rx = Some(resumed_rx);

        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            gate.checkpoint_with_hook(&cancellation, move || {
                let registered_tx = registered_tx.take();
                let resumed_rx = resumed_rx.take();
                async move {
                    if let (Some(registered_tx), Some(resumed_rx)) = (registered_tx, resumed_rx) {
                        let _ = registered_tx.send(());
                        resumed_rx.await.unwrap();
                    }
                }
            }),
        )
        .await;

        resumer.await.unwrap();

        assert_eq!(
            outcome,
            Ok(true),
            "checkpoint must observe a resume that completed between Notified registration \
             and the is_paused() check, not hang waiting for a notify_waiters() call that \
             already fired"
        );
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

    #[tokio::test]
    async fn progress_is_coalesced_not_discarded_when_full_and_critical_events_are_lossless() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 1,
            total_units: None,
            current_location: None,
        });

        // Channel is now full (capacity 1); a second progress call must
        // return immediately without blocking or panicking -- and must not
        // permanently discard this state, only coalesce it.
        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 2,
            total_units: None,
            current_location: None,
        });

        // Drain the one buffered progress event to free capacity.
        let first = rx.recv().await.unwrap();
        assert!(matches!(
            first,
            ScanEvent::Progress {
                completed_units: 1,
                ..
            }
        ));

        let sink_clone = sink.clone();
        let sender = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Paused)
                .await
                .expect("critical send must succeed");
        });

        // The coalesced progress (completed_units=2) is flushed first...
        let coalesced = rx.recv().await.unwrap();
        assert!(matches!(
            coalesced,
            ScanEvent::Progress {
                completed_units: 2,
                ..
            }
        ));

        // ...then the critical event itself, still lossless.
        let received = rx.recv().await.unwrap();
        assert!(matches!(received, ScanEvent::Paused));
        sender.await.unwrap();
    }

    /// RED reproduction for the exact counterexample: channel full, the
    /// final Progress is dropped by try_send, no further Progress event
    /// ever occurs, and a terminal event follows. Without coalescing, the
    /// frontend never receives the latest/current progress state at all --
    /// only whatever was already buffered before the drop.
    #[tokio::test]
    async fn final_progress_is_not_permanently_lost_when_the_channel_was_full() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 1,
            total_units: None,
            current_location: None,
        });

        // Channel is full; this is the final Progress this scan will ever
        // emit before terminating. try_send drops it.
        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 42,
            total_units: None,
            current_location: None,
        });

        // No further Progress event occurs. The scan settles.
        let sink_clone = sink.clone();
        let sender = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Completed {
                    visited: 42,
                    discovered: 0,
                    failure_count: 0,
                    duration_ms: 1,
                })
                .await
                .expect("critical send must succeed");
        });

        // Drain everything the sink actually sent.
        let mut received = Vec::new();
        while let Some(event) = rx.recv().await {
            let is_terminal = matches!(event, ScanEvent::Completed { .. });
            received.push(event);
            if is_terminal {
                break;
            }
        }
        sender.await.unwrap();

        let saw_current_progress = received.iter().any(|event| {
            matches!(
                event,
                ScanEvent::Progress {
                    completed_units: 42,
                    ..
                }
            )
        });
        assert!(
            saw_current_progress,
            "the frontend must eventually receive the current (completed_units=42) progress \
             state before or with the terminal event, not just the stale buffered one; got: \
             {received:?}"
        );

        let terminal_is_last = matches!(received.last(), Some(ScanEvent::Completed { .. }));
        assert!(
            terminal_is_last,
            "the current progress must be flushed before terminal settlement, not after; got: \
             {received:?}"
        );
    }

    /// Bounded memory: repeatedly dropping progress for the same
    /// `scanner_id` must replace, not accumulate -- only the single latest
    /// value ever gets flushed, never a backlog of every dropped tick.
    #[tokio::test]
    async fn coalescing_replaces_stale_progress_for_a_scanner_rather_than_accumulating_it() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        // Fill the channel once and never drain until the end, so every
        // one of these is dropped by try_send and must go through the
        // coalescing slot.
        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 0,
            total_units: None,
            current_location: None,
        });
        for completed_units in 1..1_000u64 {
            sink.progress(ScanEvent::Progress {
                scanner_id: "test".into(),
                completed_units,
                total_units: None,
                current_location: None,
            });
        }

        let sink_clone = sink.clone();
        let sender = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Completed {
                    visited: 999,
                    discovered: 0,
                    failure_count: 0,
                    duration_ms: 1,
                })
                .await
                .expect("critical send must succeed");
        });

        let mut received = Vec::new();
        while let Some(event) = rx.recv().await {
            let is_terminal = matches!(event, ScanEvent::Completed { .. });
            received.push(event);
            if is_terminal {
                break;
            }
        }
        sender.await.unwrap();

        let progress_events: Vec<_> = received
            .iter()
            .filter(|event| matches!(event, ScanEvent::Progress { .. }))
            .collect();
        // Exactly two: the one that filled the channel's own buffer
        // (completed_units=0, delivered before any drop occurred), plus the
        // single coalesced slot for the 999 subsequent drops -- not 999
        // separate accumulated events.
        assert_eq!(
            progress_events.len(),
            2,
            "999 dropped progress ticks for one scanner must coalesce down to exactly one \
             flushed event (plus the one that was already buffered before any drop), not \
             accumulate; got: {progress_events:?}"
        );
        assert!(
            matches!(
                progress_events[1],
                ScanEvent::Progress {
                    completed_units: 999,
                    ..
                }
            ),
            "the coalesced flushed event must be the latest value, not an earlier one; got: \
             {progress_events:?}"
        );
    }

    /// The coalescing slot is keyed per scanner_id, bounded by the number
    /// of distinct scanners (at most 8 for Quick), not by how many progress
    /// events were dropped -- each scanner's own latest state survives
    /// independently.
    #[tokio::test]
    async fn coalescing_is_independent_per_scanner_id() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        sink.progress(ScanEvent::Progress {
            scanner_id: "filler".into(),
            completed_units: 0,
            total_units: None,
            current_location: None,
        });
        sink.progress(ScanEvent::Progress {
            scanner_id: "a".into(),
            completed_units: 1,
            total_units: None,
            current_location: None,
        });
        sink.progress(ScanEvent::Progress {
            scanner_id: "b".into(),
            completed_units: 2,
            total_units: None,
            current_location: None,
        });

        let sink_clone = sink.clone();
        let sender = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Paused)
                .await
                .expect("critical send must succeed");
        });

        let mut received = Vec::new();
        loop {
            let event = rx.recv().await.unwrap();
            let is_terminal = matches!(event, ScanEvent::Paused);
            received.push(event);
            if is_terminal {
                break;
            }
        }
        sender.await.unwrap();

        let scanner_ids: Vec<_> = received
            .iter()
            .filter_map(|event| match event {
                ScanEvent::Progress { scanner_id, .. } => Some(scanner_id.as_str()),
                _ => None,
            })
            .collect();
        assert!(scanner_ids.contains(&"a"), "got: {received:?}");
        assert!(scanner_ids.contains(&"b"), "got: {received:?}");
    }
}
