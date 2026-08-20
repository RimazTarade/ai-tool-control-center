use crate::{Discovery, ReviewState, ScanLifecycleState, ScanScope};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
#[cfg(test)]
use rusqlite::OptionalExtension;
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Import,
    Ignore,
    KeepUnknown,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid stored discovery: {0}")]
    InvalidData(#[from] serde_json::Error),
    #[error("discovery not found")]
    NotFound,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS discoveries (
               id TEXT PRIMARY KEY,
               fingerprint TEXT NOT NULL UNIQUE,
               payload_json TEXT NOT NULL,
               review_state TEXT NOT NULL,
               observed_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS inventory (
               id TEXT PRIMARY KEY,
               discovery_id TEXT NOT NULL UNIQUE REFERENCES discoveries(id),
               payload_json TEXT NOT NULL,
               imported_at TEXT NOT NULL
             );
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
               ON scan_errors(scan_id);",
        )?;

        let has_scan_id_column = connection
            .prepare("PRAGMA table_info(discoveries)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "scan_id");
        if !has_scan_id_column {
            connection.execute_batch(
                "ALTER TABLE discoveries ADD COLUMN scan_id TEXT REFERENCES scan_runs(id);",
            )?;
        }
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_discoveries_scan_id ON discoveries(scan_id);",
        )?;

        Ok(Self { connection })
    }

    pub fn enqueue(&mut self, discovery: &Discovery) -> Result<(), StoreError> {
        let payload = serde_json::to_string(discovery)?;
        self.connection.execute(
            "INSERT INTO discoveries (id, fingerprint, payload_json, review_state, observed_at)
             VALUES (?1, ?2, ?3, 'pending', ?4)
             ON CONFLICT(fingerprint) DO UPDATE SET id=excluded.id,
               payload_json=excluded.payload_json,
               review_state='pending',
               observed_at=excluded.observed_at
             WHERE discoveries.review_state IN ('pending', 'ignored')",
            params![
                discovery.id.to_string(),
                discovery.fingerprint,
                payload,
                discovery.observed_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn pending(&self) -> Result<Vec<Discovery>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT payload_json FROM discoveries WHERE review_state='pending' ORDER BY observed_at DESC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn review(&mut self, id: Uuid, decision: ReviewDecision) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        let payload: String = transaction
            .query_row(
                "SELECT payload_json FROM discoveries WHERE id=?1 AND review_state='pending'",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound,
                other => StoreError::Database(other),
            })?;
        let state = match decision {
            ReviewDecision::Import => "imported",
            ReviewDecision::Ignore => "ignored",
            ReviewDecision::KeepUnknown => "unknown",
        };
        transaction.execute(
            "UPDATE discoveries SET review_state=?2 WHERE id=?1",
            params![id.to_string(), state],
        )?;
        if decision == ReviewDecision::Import {
            let mut discovery: Discovery = serde_json::from_str(&payload)?;
            discovery.review_state = ReviewState::Imported;
            transaction.execute(
                "INSERT INTO inventory (id, discovery_id, payload_json, imported_at) VALUES (?1, ?2, ?3, ?4)",
                params![Uuid::new_v4().to_string(), id.to_string(), serde_json::to_string(&discovery)?, chrono::Utc::now().to_rfc3339()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn inventory(&self) -> Result<Vec<Discovery>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT payload_json FROM inventory ORDER BY imported_at DESC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn begin_scan(
        &mut self,
        scan_id: Uuid,
        scope: ScanScope,
        started_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO scan_runs (id, scope, state, started_at, finished_at, failure_count)
             VALUES (?1, ?2, 'running', ?3, NULL, 0)",
            params![scan_id.to_string(), scan_scope_str(scope), started_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn set_scan_state(
        &mut self,
        scan_id: Uuid,
        state: ScanLifecycleState,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE scan_runs SET state=?2 WHERE id=?1",
            params![scan_id.to_string(), scan_lifecycle_state_str(state)],
        )?;
        Ok(())
    }

    pub fn finish_scan(
        &mut self,
        scan_id: Uuid,
        state: ScanLifecycleState,
        finished_at: DateTime<Utc>,
        failure_count: u64,
    ) -> Result<(), StoreError> {
        debug_assert!(matches!(
            state,
            ScanLifecycleState::Cancelled | ScanLifecycleState::Completed | ScanLifecycleState::Failed
        ));
        self.connection.execute(
            "UPDATE scan_runs SET state=?2, finished_at=?3, failure_count=?4 WHERE id=?1",
            params![
                scan_id.to_string(),
                scan_lifecycle_state_str(state),
                finished_at.to_rfc3339(),
                failure_count as i64
            ],
        )?;
        Ok(())
    }

    pub fn record_scan_error(
        &mut self,
        scan_id: Uuid,
        scanner_id: &str,
        code: &str,
        redacted_message: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO scan_errors (id, scan_id, scanner_id, code, redacted_message, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                scan_id.to_string(),
                scanner_id,
                code,
                redacted_message,
                observed_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn enqueue_for_scan(
        &mut self,
        scan_id: Uuid,
        discovery: &Discovery,
    ) -> Result<(), StoreError> {
        let payload = serde_json::to_string(discovery)?;
        self.connection.execute(
            "INSERT INTO discoveries (id, fingerprint, payload_json, review_state, observed_at, scan_id)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5)
             ON CONFLICT(fingerprint) DO UPDATE SET id=excluded.id,
               payload_json=excluded.payload_json,
               review_state='pending',
               observed_at=excluded.observed_at,
               scan_id=excluded.scan_id
             WHERE discoveries.review_state IN ('pending', 'ignored')",
            params![
                discovery.id.to_string(),
                discovery.fingerprint,
                payload,
                discovery.observed_at.to_rfc3339(),
                scan_id.to_string()
            ],
        )?;
        Ok(())
    }
}

fn scan_scope_str(scope: ScanScope) -> &'static str {
    match scope {
        ScanScope::Quick => "quick",
        ScanScope::Deep => "deep",
    }
}

fn scan_lifecycle_state_str(state: ScanLifecycleState) -> &'static str {
    match state {
        ScanLifecycleState::Running => "running",
        ScanLifecycleState::Paused => "paused",
        ScanLifecycleState::Cancelled => "cancelled",
        ScanLifecycleState::Completed => "completed",
        ScanLifecycleState::Failed => "failed",
    }
}

#[cfg(test)]
pub struct ScanRunRow {
    pub scope: String,
    pub state: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub failure_count: i64,
}

#[cfg(test)]
pub struct ScanErrorRow {
    pub scanner_id: String,
    pub code: String,
    pub redacted_message: String,
}

#[cfg(test)]
impl Store {
    pub fn scan_run_for_test(&self, scan_id: Uuid) -> Result<Option<ScanRunRow>, StoreError> {
        self.connection
            .query_row(
                "SELECT scope, state, started_at, finished_at, failure_count
                 FROM scan_runs WHERE id=?1",
                [scan_id.to_string()],
                |row| {
                    Ok(ScanRunRow {
                        scope: row.get(0)?,
                        state: row.get(1)?,
                        started_at: row.get(2)?,
                        finished_at: row.get(3)?,
                        failure_count: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn scan_errors_for_test(&self, scan_id: Uuid) -> Result<Vec<ScanErrorRow>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT scanner_id, code, redacted_message FROM scan_errors WHERE scan_id=?1",
        )?;
        let rows = statement.query_map([scan_id.to_string()], |row| {
            Ok(ScanErrorRow {
                scanner_id: row.get(0)?,
                code: row.get(1)?,
                redacted_message: row.get(2)?,
            })
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn discovery_scan_id_for_test(&self, id: Uuid) -> Result<Option<Uuid>, StoreError> {
        let scan_id: Option<String> = self.connection.query_row(
            "SELECT scan_id FROM discoveries WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )?;
        Ok(scan_id.map(|value| Uuid::parse_str(&value).expect("valid uuid")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Store {
        Store::in_memory().unwrap()
    }

    fn sample_discovery() -> Discovery {
        Discovery::unknown("Example", "fixture", "abc".into())
    }

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

    #[test]
    fn discoveries_require_review_before_inventory() {
        let mut store = Store::in_memory().unwrap();
        let discovery = Discovery::unknown("Example", "fixture", "abc".into());
        store.enqueue(&discovery).unwrap();
        assert_eq!(store.pending().unwrap().len(), 1);
        assert!(store.inventory().unwrap().is_empty());
        store.review(discovery.id, ReviewDecision::Import).unwrap();
        assert!(store.pending().unwrap().is_empty());
        assert_eq!(store.inventory().unwrap().len(), 1);
    }

    #[test]
    fn repeated_pending_discovery_keeps_payload_and_database_ids_aligned() {
        let mut store = Store::in_memory().unwrap();
        let first = Discovery::unknown("Example", "fixture", "same".into());
        let second = Discovery::unknown("Example", "fixture", "same".into());
        store.enqueue(&first).unwrap();
        store.enqueue(&second).unwrap();

        let pending = store.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, second.id);
        store.review(second.id, ReviewDecision::Import).unwrap();
        assert_eq!(store.inventory().unwrap().len(), 1);
    }

    #[test]
    fn ignored_discovery_can_return_on_a_later_scan() {
        let mut store = Store::in_memory().unwrap();
        let first = Discovery::unknown("Example", "fixture", "same".into());
        store.enqueue(&first).unwrap();
        store.review(first.id, ReviewDecision::Ignore).unwrap();
        assert!(store.pending().unwrap().is_empty());

        let later = Discovery::unknown("Example", "fixture", "same".into());
        store.enqueue(&later).unwrap();
        assert_eq!(store.pending().unwrap()[0].id, later.id);
    }
}
