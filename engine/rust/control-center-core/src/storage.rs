use crate::{Discovery, ReviewState};
use rusqlite::{Connection, params};
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
             );",
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
