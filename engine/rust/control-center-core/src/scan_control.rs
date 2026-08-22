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
/// A request to send a lossless "critical" event (discovery, lifecycle
/// change, `ScannerFailed`, or terminal settlement), routed to the pump
/// task so that draining coalesced progress and enqueuing the critical
/// event itself happen as one atomic step from the perspective of any
/// concurrent `progress()` caller (see `run_progress_pump`).
struct CriticalRequest {
    event: ScanEvent,
    ack: tokio::sync::oneshot::Sender<Result<(), mpsc::error::SendError<ScanEvent>>>,
}

/// Whether the coalescing slot is still accepting new progress. Closed the
/// instant a terminal event is drained by the pump, under the same lock
/// acquisition that performs the drain -- so there is no window between
/// "the map was last read" and "the terminal event was enqueued" during
/// which a `progress()` call can insert something that will never be sent
/// (see `run_progress_pump`'s handling of `CriticalRequest`).
enum CoalesceState {
    Open(HashMap<String, ScanEvent>),
    Closed,
}

/// The sole task that writes to the real output channel. Making delivery
/// single-writer/sequential-by-construction is what eliminates both the
/// stale-reordering race and the drain/terminal race that a second
/// "check again" pass around a shared `HashMap` cannot close: a caller
/// racing an in-progress drain always resolves to "arrived before the
/// drain" (goes out with it) or "arrived after the state flipped to
/// `Closed`" (a no-op) because both the drain and the `Closed` transition
/// happen under one lock acquisition here, and this task is the only
/// place that ever performs a drain.
async fn run_progress_pump<F, Fut>(
    tx: mpsc::Sender<ScanEvent>,
    coalesce_state: Arc<Mutex<CoalesceState>>,
    progress_notify: Arc<Notify>,
    mut critical_rx: mpsc::UnboundedReceiver<CriticalRequest>,
    mut after_drain: F,
) where
    F: FnMut() -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    loop {
        tokio::select! {
            request = critical_rx.recv() => {
                let Some(request) = request else { break };
                let is_terminal = matches!(
                    request.event,
                    ScanEvent::Cancelled { .. } | ScanEvent::Completed { .. } | ScanEvent::Failed { .. }
                );

                let drained: Vec<ScanEvent> = {
                    let mut state = coalesce_state.lock().unwrap();
                    match &mut *state {
                        CoalesceState::Open(map) => {
                            let drained: Vec<ScanEvent> =
                                map.drain().map(|(_, event)| event).collect();
                            if is_terminal {
                                *state = CoalesceState::Closed;
                            }
                            drained
                        }
                        CoalesceState::Closed => Vec::new(),
                    }
                };

                // Test-only seam: production always passes a no-op hook
                // that resolves without yielding. The test module below
                // uses it to deterministically hold the pump exactly here
                // -- state already committed (drained, and `Closed` if
                // terminal), but before any of the drained progress or the
                // critical event itself has been sent -- to reproduce the
                // exact interleaving a concurrent `progress()` call could
                // race into, without relying on scheduling luck or sleeps.
                after_drain().await;

                let mut result = Ok(());
                for progress in drained {
                    if let Err(error) = tx.send(progress).await {
                        result = Err(error);
                        break;
                    }
                }
                if result.is_ok() {
                    result = tx.send(request.event).await;
                }
                let receiver_gone = result.is_err();
                let _ = request.ack.send(result);
                if is_terminal || receiver_gone {
                    break;
                }
            }
            () = progress_notify.notified() => {
                // Opportunistic live forwarding: try to drain whatever is
                // currently coalesced without blocking. Anything that
                // still doesn't fit stays in the map -- it is still
                // guaranteed to reach the frontend, either by a later
                // notification once capacity frees up, or unconditionally
                // (lossless) whenever the next `CriticalRequest` arrives.
                let mut state = coalesce_state.lock().unwrap();
                if let CoalesceState::Open(map) = &mut *state {
                    let scanner_ids: Vec<String> = map.keys().cloned().collect();
                    for scanner_id in scanner_ids {
                        let Some(event) = map.get(&scanner_id).cloned() else {
                            continue;
                        };
                        if tx.try_send(event).is_ok() {
                            map.remove(&scanner_id);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct ScanEventSink {
    failures: Arc<AtomicU64>,
    /// Latest not-yet-forwarded `Progress` event per `scanner_id`. Always
    /// the only path progress takes to the wire -- see `run_progress_pump`.
    coalesce_state: Arc<Mutex<CoalesceState>>,
    /// Wakes the pump to opportunistically forward newly-coalesced
    /// progress without waiting for the next critical/terminal event.
    progress_notify: Arc<Notify>,
    /// Requests handled strictly sequentially by the pump task, making the
    /// drain-then-send-then-close sequence atomic relative to `progress()`.
    critical_tx: mpsc::UnboundedSender<CriticalRequest>,
}

impl ScanEventSink {
    pub fn new(tx: mpsc::Sender<ScanEvent>) -> Self {
        Self::new_with_after_drain_hook(tx, || std::future::ready(()))
    }

    /// Core constructor, parameterized over a hook invoked exactly once per
    /// processed `CriticalRequest`, immediately after the atomic
    /// drain-and-maybe-close step and before anything is actually sent
    /// (see `run_progress_pump`). Production always calls this via `new()`
    /// with a no-op hook; the test module below uses an async hook to
    /// deterministically drive a concurrent `progress()` into that exact
    /// window, reproducing the tightest possible interleaving without
    /// relying on real scheduling luck or sleeps.
    fn new_with_after_drain_hook<F, Fut>(tx: mpsc::Sender<ScanEvent>, after_drain: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let coalesce_state = Arc::new(Mutex::new(CoalesceState::Open(HashMap::new())));
        let progress_notify = Arc::new(Notify::new());
        let (critical_tx, critical_rx) = mpsc::unbounded_channel();

        tokio::spawn(run_progress_pump(
            tx,
            coalesce_state.clone(),
            progress_notify.clone(),
            critical_rx,
            after_drain,
        ));

        Self {
            failures: Arc::new(AtomicU64::new(0)),
            coalesce_state,
            progress_notify,
            critical_tx,
        }
    }

    /// Best-effort send for high-frequency progress events. Never blocks:
    /// only locks a `std::sync::Mutex` for an insert (no `.await` while
    /// holding it) and fires a `Notify`. Always replaces any previous
    /// not-yet-forwarded progress for the same `scanner_id` -- this is the
    /// *only* path progress ever takes to the wire, so there is no second,
    /// uncoordinated delivery path that a newer event could race past a
    /// stale one on.
    pub fn progress(&self, event: ScanEvent) {
        debug_assert!(matches!(event, ScanEvent::Progress { .. }));
        let ScanEvent::Progress { scanner_id, .. } = &event else {
            return;
        };
        let scanner_id = scanner_id.clone();
        let mut state = self.coalesce_state.lock().unwrap();
        match &mut *state {
            CoalesceState::Open(map) => {
                map.insert(scanner_id, event);
                drop(state);
                self.progress_notify.notify_one();
            }
            // The pump already committed to terminal settlement; nothing
            // will ever drain this map again, so inserting would just leak
            // memory for the remaining lifetime of the sink.
            CoalesceState::Closed => {}
        }
    }

    /// Lossless send for discoveries, lifecycle changes and terminal
    /// events. Routed through the pump task (see `run_progress_pump`) so
    /// that draining any coalesced progress and enqueuing `event` happen
    /// as a single atomic step relative to concurrent `progress()` calls.
    pub async fn critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        let request = CriticalRequest {
            event: event.clone(),
            ack: ack_tx,
        };
        if self.critical_tx.send(request).is_err() {
            // Pump task is gone (channel already closed / prior terminal
            // event already settled it) -- nothing left to send to.
            return Err(mpsc::error::SendError(event));
        }
        ack_rx.await.unwrap_or(Err(mpsc::error::SendError(event)))
    }

    /// Lossless send for use from a blocking (non-async) context, such as
    /// a `spawn_blocking` filesystem walk. Blocks the current thread on a
    /// oneshot ack from the pump task rather than dropping the event.
    ///
    /// The error is boxed (unlike the async sibling above, which is not
    /// flagged by `clippy::result_large_err` since it does not fire on
    /// `async fn`) because `SendError<ScanEvent>` embeds a whole `ScanEvent`
    /// and is large enough to make every caller's `Result` oversized even
    /// when the call succeeds.
    pub fn blocking_critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), Box<mpsc::error::SendError<ScanEvent>>> {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        let request = CriticalRequest {
            event: event.clone(),
            ack: ack_tx,
        };
        if self.critical_tx.send(request).is_err() {
            return Err(Box::new(mpsc::error::SendError(event)));
        }
        match ack_rx.blocking_recv() {
            Ok(result) => result.map_err(Box::new),
            Err(_) => Err(Box::new(mpsc::error::SendError(event))),
        }
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

    /// Test-only introspection: the number of scanner ids currently
    /// holding an undelivered coalesced progress event, or `None` once the
    /// slot has closed after terminal settlement. Used to assert bounded
    /// memory / no stranding directly, rather than only inferring it from
    /// what did or didn't arrive on the channel.
    #[cfg(test)]
    fn coalesced_len(&self) -> Option<usize> {
        match &*self.coalesce_state.lock().unwrap() {
            CoalesceState::Open(map) => Some(map.len()),
            CoalesceState::Closed => None,
        }
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

        // Let the pump task opportunistically forward this first progress
        // event through the channel's own buffer (capacity 1).
        let first = rx.recv().await.unwrap();
        assert!(matches!(
            first,
            ScanEvent::Progress {
                completed_units: 1,
                ..
            }
        ));

        // A second progress call must return immediately without blocking
        // or panicking -- and must not permanently discard this state,
        // only coalesce it.
        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
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

        // None of these are ever awaited on, so the pump task never gets a
        // chance to run between them -- all 1000 land in the same
        // coalescing slot for "test", each overwriting the last.
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
        // Exactly one: every one of the 1000 progress() calls landed in the
        // same per-scanner coalescing slot before the pump ever ran, so
        // only the single latest value is ever flushed -- not 1000
        // separate accumulated events.
        assert_eq!(
            progress_events.len(),
            1,
            "1000 coalesced progress ticks for one scanner must collapse down to exactly one \
             flushed event, not accumulate; got: {progress_events:?}"
        );
        assert!(
            matches!(
                progress_events[0],
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

    fn progress(scanner_id: &str, completed_units: u64) -> ScanEvent {
        ScanEvent::Progress {
            scanner_id: scanner_id.into(),
            completed_units,
            total_units: None,
            current_location: None,
        }
    }

    /// RACE 1 (stale coalesced progress overtaking a newer directly-sent
    /// one): deterministically forces the tightest possible interleaving
    /// between two concurrent producers racing to update the same
    /// scanner_id's coalesced slot, using a oneshot barrier rather than
    /// sleeps. `progress()` has exactly one delivery path -- the
    /// coalescing slot -- so a "newer" value can never reach the wire via
    /// a second, uncoordinated path ahead of an "older" one still sitting
    /// in the slot: whichever write actually lands last in the slot is
    /// unconditionally what gets flushed, regardless of arrival order.
    #[tokio::test]
    async fn a_newer_progress_write_can_never_be_overtaken_by_a_stale_one_for_the_same_scanner() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        let (p2_wrote_tx, p2_wrote_rx) = tokio::sync::oneshot::channel::<()>();

        let p2_sink = sink.clone();
        let p2 = tokio::spawn(async move {
            p2_sink.progress(progress("test", 2));
            let _ = p2_wrote_tx.send(());
        });

        let p3_sink = sink.clone();
        let p3 = tokio::spawn(async move {
            // P3 (the newer write) is forced to land strictly after P2 (the
            // older write) has already landed in the coalescing slot --
            // the exact ordering the original bug depended on to let a
            // stale value survive behind a fresher one.
            p2_wrote_rx.await.unwrap();
            p3_sink.progress(progress("test", 3));
        });

        p2.await.unwrap();
        p3.await.unwrap();

        // Spawned rather than awaited inline: the channel has capacity 1,
        // so sending both the drained progress and the terminal event can
        // require the receiver (below) to drain concurrently.
        let sink_clone = sink.clone();
        let sender = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Completed {
                    visited: 3,
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

        let progress_values: Vec<u64> = received
            .iter()
            .filter_map(|event| match event {
                ScanEvent::Progress {
                    completed_units, ..
                } => Some(*completed_units),
                _ => None,
            })
            .collect();

        assert_eq!(
            progress_values,
            vec![3],
            "the older write (2) must never be delivered after the newer one (3) settled in \
             the same coalescing slot -- progress must never regress; got: {received:?}"
        );
    }

    /// RACE 2 (progress stranded behind terminal settlement): deterministic
    /// reproduction using the `after_drain` test hook to hold the pump
    /// exactly between "drained the coalescing slot and (for a terminal
    /// event) flipped it to `Closed`" and "actually sent anything" --
    /// the precise window the original bug left open between the drain and
    /// the terminal enqueue. A concurrent `progress()` call forced into
    /// that exact window must be a safe, bounded no-op (the slot is
    /// already `Closed`), never a value that is accepted but then never
    /// sent -- i.e. never a memory leak, and never delivered out of order
    /// after the terminal event.
    #[tokio::test]
    async fn no_progress_is_stranded_when_it_arrives_during_the_atomic_drain_and_close() {
        let (tx, mut rx) = mpsc::channel(1);

        let (drained_tx, drained_rx) = tokio::sync::oneshot::channel::<()>();
        let (proceed_tx, proceed_rx) = tokio::sync::oneshot::channel::<()>();
        let mut drained_tx = Some(drained_tx);
        let mut proceed_rx = Some(proceed_rx);

        let sink = ScanEventSink::new_with_after_drain_hook(tx, move || {
            let drained_tx = drained_tx.take();
            let proceed_rx = proceed_rx.take();
            async move {
                if let (Some(drained_tx), Some(proceed_rx)) = (drained_tx, proceed_rx) {
                    let _ = drained_tx.send(());
                    proceed_rx.await.unwrap();
                }
            }
        });

        // Seed the coalescing slot with P2, the value the terminal drain
        // will pick up.
        sink.progress(progress("test", 2));

        let sink_clone = sink.clone();
        let critical_task = tokio::spawn(async move {
            sink_clone
                .critical(ScanEvent::Completed {
                    visited: 2,
                    discovered: 0,
                    failure_count: 0,
                    duration_ms: 1,
                })
                .await
        });

        // Wait until the pump has atomically drained {2} and (since this is
        // a terminal event) flipped the slot to `Closed` -- but has not yet
        // sent anything.
        drained_rx.await.unwrap();

        // A concurrent producer emits P3 for the same scanner while the
        // pump is paused in that exact window.
        sink.progress(progress("test", 3));

        // Let the pump proceed: send the drained P2, then the terminal
        // event.
        let _ = proceed_tx.send(());

        let first = rx.recv().await.unwrap();
        assert!(
            matches!(
                first,
                ScanEvent::Progress {
                    completed_units: 2,
                    ..
                }
            ),
            "got: {first:?}"
        );
        let second = rx.recv().await.unwrap();
        assert!(
            matches!(second, ScanEvent::Completed { .. }),
            "the terminal event must follow immediately -- no progress may be stranded between \
             the drain and terminal settlement; got: {second:?}"
        );

        critical_task
            .await
            .unwrap()
            .expect("critical send must succeed");

        // P3 must not have leaked into a slot nothing will ever drain
        // again: the coalescing slot is closed for good after terminal
        // settlement, so a further progress() call is a safe, bounded
        // no-op -- never silent unbounded growth.
        assert_eq!(
            sink.coalesced_len(),
            None,
            "the coalescing slot must be Closed (not silently retaining P3) once terminal \
             settlement has drained and started sending"
        );
        sink.progress(progress("test", 4));
        assert_eq!(
            sink.coalesced_len(),
            None,
            "a progress() call after terminal settlement must remain a no-op, not reopen or \
             grow the slot"
        );
    }
}
