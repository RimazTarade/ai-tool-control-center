use crate::Discovery;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
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

/// Bounded sink for scan events. `Progress` events are best-effort and may
/// be dropped when the channel is full; every other event class is
/// lossless (awaited `send`).
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

    /// Best-effort send for high-frequency progress events. Never blocks;
    /// silently drops the event if the channel is full.
    pub fn progress(&self, event: ScanEvent) {
        debug_assert!(matches!(event, ScanEvent::Progress { .. }));
        let _ = self.tx.try_send(event);
    }

    /// Lossless send for discoveries, lifecycle changes and terminal
    /// events. Awaits channel capacity rather than dropping.
    pub async fn critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        self.tx.send(event).await
    }

    /// Lossless send for use from a blocking (non-async) context, such as
    /// a `spawn_blocking` filesystem walk. Blocks the current thread until
    /// channel capacity is available rather than dropping the event.
    pub fn blocking_critical(
        &self,
        event: ScanEvent,
    ) -> Result<(), mpsc::error::SendError<ScanEvent>> {
        self.tx.blocking_send(event)
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
    async fn progress_is_dropped_when_full_but_critical_events_are_lossless() {
        let (tx, mut rx) = mpsc::channel(1);
        let sink = ScanEventSink::new(tx);

        sink.progress(ScanEvent::Progress {
            scanner_id: "test".into(),
            completed_units: 1,
            total_units: None,
            current_location: None,
        });

        // Channel is now full (capacity 1); a second progress call must
        // return immediately without blocking or panicking.
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

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, ScanEvent::Paused));
        sender.await.unwrap();
    }
}
