use crate::{
    HealthCheckDefinition, HealthProbeOutcome, HealthResult, HealthRunScope, HealthState,
    NativeHealthProbeIo, execute_health_check,
};
use chrono::Utc;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const HEALTH_INSTALLATION_CONCURRENCY: usize = 4;

pub trait HealthCheckExecutor: Send + Sync {
    fn execute(&self, definition: &HealthCheckDefinition) -> HealthProbeOutcome;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeHealthCheckExecutor {
    io: NativeHealthProbeIo,
}

impl HealthCheckExecutor for NativeHealthCheckExecutor {
    fn execute(&self, definition: &HealthCheckDefinition) -> HealthProbeOutcome {
        execute_health_check(definition, &self.io)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthEvent {
    Started {
        run_id: Uuid,
        scope: HealthRunScope,
        requested_installation_count: u64,
    },
    CheckCompleted {
        run_id: Uuid,
        result: HealthResult,
    },
    Cancelled {
        run_id: Uuid,
        completed_installation_count: u64,
    },
    Completed {
        run_id: Uuid,
        completed_installation_count: u64,
    },
}

#[derive(Clone)]
pub struct HealthEventSink {
    tx: mpsc::Sender<HealthEvent>,
}

impl HealthEventSink {
    pub fn new(tx: mpsc::Sender<HealthEvent>) -> Self {
        Self { tx }
    }

    async fn send(&self, event: HealthEvent) -> Result<(), mpsc::error::SendError<HealthEvent>> {
        self.tx.send(event).await
    }
}

#[derive(Clone, Debug)]
pub struct InstallationHealthWork {
    pub installation_id: Uuid,
    pub definitions: Vec<HealthCheckDefinition>,
}

pub async fn run_health_work(
    run_id: Uuid,
    scope: HealthRunScope,
    work: Vec<InstallationHealthWork>,
    executor: Arc<dyn HealthCheckExecutor>,
    cancellation: CancellationToken,
    events: HealthEventSink,
) {
    let requested_installation_count = u64::try_from(work.len()).unwrap_or(u64::MAX);
    if events
        .send(HealthEvent::Started {
            run_id,
            scope,
            requested_installation_count,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut pending = VecDeque::from(work);
    let mut workers = JoinSet::new();
    let mut completed_installation_count = 0_u64;
    let mut cancelled = cancellation.is_cancelled();

    loop {
        while !cancelled && workers.len() < HEALTH_INSTALLATION_CONCURRENCY && !pending.is_empty() {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }

            let item = pending.pop_front().expect("pending work checked non-empty");
            let worker_executor = executor.clone();
            let worker_cancellation = cancellation.clone();
            let worker_events = events.clone();
            workers.spawn(async move {
                run_installation_work(
                    run_id,
                    item,
                    worker_executor,
                    worker_cancellation,
                    worker_events,
                )
                .await
            });
        }

        if workers.is_empty() {
            break;
        }

        if cancelled {
            while let Some(joined) = workers.join_next().await {
                if matches!(joined, Ok(true)) {
                    completed_installation_count += 1;
                }
            }
            break;
        }

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                cancelled = true;
            }
            joined = workers.join_next() => {
                if matches!(joined, Some(Ok(true))) {
                    completed_installation_count += 1;
                }
            }
        }
    }

    if cancellation.is_cancelled() || cancelled {
        let _ = events
            .send(HealthEvent::Cancelled {
                run_id,
                completed_installation_count,
            })
            .await;
    } else {
        let _ = events
            .send(HealthEvent::Completed {
                run_id,
                completed_installation_count,
            })
            .await;
    }
}

async fn run_installation_work(
    run_id: Uuid,
    mut work: InstallationHealthWork,
    executor: Arc<dyn HealthCheckExecutor>,
    cancellation: CancellationToken,
    events: HealthEventSink,
) -> bool {
    work.definitions.sort_by_key(|definition| definition.id);

    for definition in work.definitions {
        if cancellation.is_cancelled() {
            return false;
        }

        let definition_for_probe = definition.clone();
        let probe_executor = executor.clone();
        let started_at = Instant::now();
        let blocking =
            tokio::task::spawn_blocking(move || probe_executor.execute(&definition_for_probe));
        let timed = tokio::time::timeout(Duration::from_millis(definition.timeout_ms), blocking);
        tokio::pin!(timed);

        let outcome = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return false;
            }
            joined = &mut timed => {
                match joined {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(_)) => HealthProbeOutcome {
                        state: HealthState::Grey,
                        result_code: "health_check_executor_failed".into(),
                        redacted_evidence: "health check executor failed".into(),
                    },
                    Err(_) => HealthProbeOutcome {
                        state: HealthState::Grey,
                        result_code: "health_check_timeout".into(),
                        redacted_evidence: "health check timed out".into(),
                    },
                }
            }
        };

        let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        let result = HealthResult {
            id: Uuid::new_v4(),
            run_id,
            installation_id: work.installation_id,
            check_definition_id: definition.id,
            check_type: definition.check_type.clone(),
            required: definition.required,
            state: outcome.state,
            result_code: outcome.result_code,
            redacted_evidence: outcome.redacted_evidence,
            duration_ms,
            observed_at: Utc::now(),
        };

        if events
            .send(HealthEvent::CheckCompleted { run_id, result })
            .await
            .is_err()
        {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        HealthCheckDefinition, HealthCheckType, HealthExpectedCondition, HealthProbeOutcome,
        HealthRunScope, HealthState, HealthTarget,
    };
    use chrono::Utc;
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::sync::{Notify, Semaphore, mpsc};
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        wake: Condvar,
    }

    impl Gate {
        fn wait(&self) {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.wake.wait(open).unwrap();
            }
        }

        fn open(&self) {
            *self.open.lock().unwrap() = true;
            self.wake.notify_all();
        }
    }

    struct FakeExecutor {
        handler: Arc<dyn Fn(&HealthCheckDefinition) -> HealthProbeOutcome + Send + Sync>,
    }

    impl HealthCheckExecutor for FakeExecutor {
        fn execute(&self, definition: &HealthCheckDefinition) -> HealthProbeOutcome {
            (self.handler)(definition)
        }
    }

    fn fake_executor<F>(handler: F) -> Arc<dyn HealthCheckExecutor>
    where
        F: Fn(&HealthCheckDefinition) -> HealthProbeOutcome + Send + Sync + 'static,
    {
        Arc::new(FakeExecutor {
            handler: Arc::new(handler),
        })
    }

    fn definition(id: u128, installation_id: Uuid, timeout_ms: u64) -> HealthCheckDefinition {
        let now = Utc::now();
        HealthCheckDefinition {
            id: Uuid::from_u128(id),
            installation_id,
            check_type: HealthCheckType::PathExists,
            required: true,
            expected_condition: HealthExpectedCondition::Exists,
            timeout_ms,
            stale_after_seconds: None,
            target_metadata: HealthTarget::Path {
                path: PathBuf::from(format!("C:\\health\\{id}.exe")),
            },
            created_at: now,
            updated_at: now,
        }
    }

    fn outcome(state: HealthState, code: &str) -> HealthProbeOutcome {
        HealthProbeOutcome {
            state,
            result_code: code.into(),
            redacted_evidence: "fixture".into(),
        }
    }

    fn work(
        installation_id: Uuid,
        definitions: Vec<HealthCheckDefinition>,
    ) -> InstallationHealthWork {
        InstallationHealthWork {
            installation_id,
            definitions,
        }
    }

    async fn collect_run(
        work_items: Vec<InstallationHealthWork>,
        executor: Arc<dyn HealthCheckExecutor>,
        cancellation: CancellationToken,
    ) -> Vec<HealthEvent> {
        let (tx, mut rx) = mpsc::channel(64);
        run_health_work(
            Uuid::new_v4(),
            HealthRunScope::All,
            work_items,
            executor,
            cancellation,
            HealthEventSink::new(tx),
        )
        .await;

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn health_control_check_timeout_does_not_block_later_checks() {
        let installation_id = Uuid::new_v4();
        let blocked_id = Uuid::from_u128(1);
        let later_id = Uuid::from_u128(2);
        let gate = Arc::new(Gate::default());
        let gate_for_executor = gate.clone();
        let executor = fake_executor(move |definition| {
            if definition.id == blocked_id {
                gate_for_executor.wait();
            }
            outcome(HealthState::Green, "ok")
        });

        let events = collect_run(
            vec![work(
                installation_id,
                vec![
                    definition(1, installation_id, 25),
                    definition(2, installation_id, 250),
                ],
            )],
            executor,
            CancellationToken::new(),
        )
        .await;
        gate.open();

        let results: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                HealthEvent::CheckCompleted { result, .. } => Some(result),
                _ => None,
            })
            .collect();

        assert_eq!(results.len(), 2);
        let timed_out = results
            .iter()
            .find(|result| result.check_definition_id == blocked_id)
            .unwrap();
        assert_eq!(timed_out.state, HealthState::Grey);
        assert_eq!(timed_out.result_code, "health_check_timeout");
        assert!(
            results
                .iter()
                .any(|result| result.check_definition_id == later_id
                    && result.state == HealthState::Green)
        );
    }

    #[tokio::test]
    async fn health_control_slow_installation_does_not_block_unrelated_installation() {
        let slow_installation = Uuid::new_v4();
        let fast_installation = Uuid::new_v4();
        let slow_definition = definition(10, slow_installation, 5_000);
        let fast_definition = definition(20, fast_installation, 5_000);
        let slow_id = slow_definition.id;
        let fast_id = fast_definition.id;
        let gate = Arc::new(Gate::default());
        let started = Arc::new(Notify::new());
        let gate_for_executor = gate.clone();
        let started_for_executor = started.clone();
        let executor = fake_executor(move |definition| {
            if definition.id == slow_id {
                started_for_executor.notify_one();
                gate_for_executor.wait();
            }
            outcome(HealthState::Green, "ok")
        });
        let cancellation = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(16);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::All,
            vec![
                work(slow_installation, vec![slow_definition]),
                work(fast_installation, vec![fast_definition]),
            ],
            executor,
            cancellation,
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        started.notified().await;
        let next = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(next, HealthEvent::CheckCompleted { result, .. } if result.check_definition_id == fast_id)
        );

        gate.open();
        run.await.unwrap();
    }

    #[tokio::test]
    async fn health_control_check_all_never_exceeds_four_installations() {
        let current = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Semaphore::new(0));
        let gate = Arc::new(Gate::default());
        let current_for_executor = current.clone();
        let maximum_for_executor = maximum.clone();
        let started_for_executor = started.clone();
        let gate_for_executor = gate.clone();
        let executor = fake_executor(move |_| {
            let active = current_for_executor.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_for_executor.fetch_max(active, Ordering::SeqCst);
            started_for_executor.add_permits(1);
            gate_for_executor.wait();
            current_for_executor.fetch_sub(1, Ordering::SeqCst);
            outcome(HealthState::Green, "ok")
        });

        let mut work_items = Vec::new();
        for index in 0_u128..8 {
            let installation_id = Uuid::new_v4();
            work_items.push(work(
                installation_id,
                vec![definition(100 + index, installation_id, 5_000)],
            ));
        }

        let (tx, mut rx) = mpsc::channel(32);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::All,
            work_items,
            executor,
            CancellationToken::new(),
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        let permits = started.acquire_many(4).await.unwrap();
        permits.forget();
        assert_eq!(
            current.load(Ordering::SeqCst),
            HEALTH_INSTALLATION_CONCURRENCY
        );
        assert_eq!(
            maximum.load(Ordering::SeqCst),
            HEALTH_INSTALLATION_CONCURRENCY
        );

        gate.open();
        run.await.unwrap();
        assert!(maximum.load(Ordering::SeqCst) <= HEALTH_INSTALLATION_CONCURRENCY);
    }

    #[tokio::test]
    async fn health_control_cancellation_preserves_already_completed_results() {
        let installation_id = Uuid::new_v4();
        let first = definition(200, installation_id, 5_000);
        let second = definition(201, installation_id, 5_000);
        let first_id = first.id;
        let second_id = second.id;
        let gate = Arc::new(Gate::default());
        let second_started = Arc::new(Notify::new());
        let gate_for_executor = gate.clone();
        let second_started_for_executor = second_started.clone();
        let executor = fake_executor(move |definition| {
            if definition.id == second_id {
                second_started_for_executor.notify_one();
                gate_for_executor.wait();
            }
            outcome(HealthState::Green, "ok")
        });
        let cancellation = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(16);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::Installation,
            vec![work(installation_id, vec![first, second])],
            executor,
            cancellation.clone(),
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        let first_event = rx.recv().await.unwrap();
        assert!(
            matches!(&first_event, HealthEvent::CheckCompleted { result, .. } if result.check_definition_id == first_id)
        );
        second_started.notified().await;
        cancellation.cancel();
        let terminal = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(terminal, HealthEvent::Cancelled { .. }));

        gate.open();
        run.await.unwrap();
    }

    #[tokio::test]
    async fn health_control_cancellation_prevents_new_checks() {
        let installation_id = Uuid::new_v4();
        let first = definition(300, installation_id, 5_000);
        let second = definition(301, installation_id, 5_000);
        let first_id = first.id;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_executor = calls.clone();
        let cancellation = CancellationToken::new();
        let cancellation_for_executor = cancellation.clone();
        let executor = fake_executor(move |definition| {
            calls_for_executor.lock().unwrap().push(definition.id);
            if definition.id == first_id {
                cancellation_for_executor.cancel();
            }
            outcome(HealthState::Green, "ok")
        });

        let events = collect_run(
            vec![work(installation_id, vec![first, second])],
            executor,
            cancellation,
        )
        .await;

        assert_eq!(*calls.lock().unwrap(), vec![first_id]);
        assert!(matches!(events.last(), Some(HealthEvent::Cancelled { .. })));
    }

    #[tokio::test]
    async fn health_control_cancellation_prevents_new_installation_admission() {
        let started_count = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Semaphore::new(0));
        let gate = Arc::new(Gate::default());
        let started_count_for_executor = started_count.clone();
        let started_for_executor = started.clone();
        let gate_for_executor = gate.clone();
        let executor = fake_executor(move |_| {
            started_count_for_executor.fetch_add(1, Ordering::SeqCst);
            started_for_executor.add_permits(1);
            gate_for_executor.wait();
            outcome(HealthState::Green, "ok")
        });

        let mut work_items = Vec::new();
        for index in 0_u128..8 {
            let installation_id = Uuid::new_v4();
            work_items.push(work(
                installation_id,
                vec![definition(400 + index, installation_id, 5_000)],
            ));
        }

        let cancellation = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(32);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::All,
            work_items,
            executor,
            cancellation.clone(),
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        let permits = started.acquire_many(4).await.unwrap();
        permits.forget();
        cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(1), run)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            started_count.load(Ordering::SeqCst),
            HEALTH_INSTALLATION_CONCURRENCY
        );
        gate.open();
    }

    #[tokio::test]
    async fn health_control_late_blocking_outcome_after_cancel_is_discarded() {
        let installation_id = Uuid::new_v4();
        let definition = definition(500, installation_id, 5_000);
        let gate = Arc::new(Gate::default());
        let started = Arc::new(Notify::new());
        let gate_for_executor = gate.clone();
        let started_for_executor = started.clone();
        let executor = fake_executor(move |_| {
            started_for_executor.notify_one();
            gate_for_executor.wait();
            outcome(HealthState::Green, "late")
        });
        let cancellation = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(16);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::Installation,
            vec![work(installation_id, vec![definition])],
            executor,
            cancellation.clone(),
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        started.notified().await;
        cancellation.cancel();
        let terminal = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(terminal, HealthEvent::Cancelled { .. }));
        assert!(rx.try_recv().is_err());

        gate.open();
        run.await.unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn health_control_result_states_do_not_fail_batch() {
        let installation_id = Uuid::new_v4();
        let red = definition(600, installation_id, 5_000);
        let blue = definition(601, installation_id, 5_000);
        let grey = definition(602, installation_id, 5_000);
        let states = Arc::new(HashMap::from([
            (red.id, HealthState::Red),
            (blue.id, HealthState::Blue),
            (grey.id, HealthState::Grey),
        ]));
        let states_for_executor = states.clone();
        let executor = fake_executor(move |definition| {
            outcome(
                states_for_executor.get(&definition.id).unwrap().clone(),
                "fixture_state",
            )
        });

        let events = collect_run(
            vec![work(installation_id, vec![red, blue, grey])],
            executor,
            CancellationToken::new(),
        )
        .await;
        let result_count = events
            .iter()
            .filter(|event| matches!(event, HealthEvent::CheckCompleted { .. }))
            .count();

        assert_eq!(result_count, 3);
        assert!(matches!(events.last(), Some(HealthEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn health_control_terminal_event_is_last() {
        let completed_installation = Uuid::new_v4();
        let completed_events = collect_run(
            vec![work(
                completed_installation,
                vec![definition(700, completed_installation, 5_000)],
            )],
            fake_executor(|_| outcome(HealthState::Green, "ok")),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            completed_events.last(),
            Some(HealthEvent::Completed { .. })
        ));

        let cancelled_installation = Uuid::new_v4();
        let gate = Arc::new(Gate::default());
        let started = Arc::new(Notify::new());
        let gate_for_executor = gate.clone();
        let started_for_executor = started.clone();
        let executor = fake_executor(move |_| {
            started_for_executor.notify_one();
            gate_for_executor.wait();
            outcome(HealthState::Green, "late")
        });
        let cancellation = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(16);
        let run = tokio::spawn(run_health_work(
            Uuid::new_v4(),
            HealthRunScope::Installation,
            vec![work(
                cancelled_installation,
                vec![definition(701, cancelled_installation, 5_000)],
            )],
            executor,
            cancellation.clone(),
            HealthEventSink::new(tx),
        ));

        assert!(matches!(
            rx.recv().await.unwrap(),
            HealthEvent::Started { .. }
        ));
        started.notified().await;
        cancellation.cancel();
        run.await.unwrap();
        let mut cancelled_events = Vec::new();
        while let Some(event) = rx.recv().await {
            cancelled_events.push(event);
        }
        gate.open();

        assert!(matches!(
            cancelled_events.last(),
            Some(HealthEvent::Cancelled { .. })
        ));
        assert!(
            !cancelled_events
                .iter()
                .skip_while(|event| !matches!(event, HealthEvent::Cancelled { .. }))
                .skip(1)
                .any(|event| matches!(event, HealthEvent::CheckCompleted { .. }))
        );
    }

    #[tokio::test]
    async fn health_control_definition_order_is_deterministic_within_installation() {
        let installation_id = Uuid::new_v4();
        let first = definition(800, installation_id, 5_000);
        let second = definition(801, installation_id, 5_000);
        let third = definition(802, installation_id, 5_000);
        let expected = vec![first.id, second.id, third.id];
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_executor = calls.clone();
        let executor = fake_executor(move |definition| {
            calls_for_executor.lock().unwrap().push(definition.id);
            outcome(HealthState::Green, "ok")
        });

        collect_run(
            vec![work(installation_id, vec![third, second, first])],
            executor,
            CancellationToken::new(),
        )
        .await;

        assert_eq!(*calls.lock().unwrap(), expected);
    }
}
