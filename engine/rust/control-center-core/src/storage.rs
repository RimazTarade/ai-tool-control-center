use crate::{
    CanonicalCapability, Discovery, HealthCheckDefinition, HealthState, HealthTarget,
    InstallationInstance, ReviewState, ScanLifecycleState, ScanScope,
};
use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
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
    #[error("invalid stored data: {0}")]
    StorageIntegrity(String),
    #[error("discovery not found")]
    NotFound,
    #[error("finish_scan requires a terminal state (Cancelled, Completed, or Failed), got {0:?}")]
    NonTerminalFinishState(ScanLifecycleState),
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

    fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
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

        migrate_m7_health_dependency_systems(&mut connection)?;
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
            let imported_at = Utc::now().to_rfc3339();

            transaction.execute(
                "INSERT INTO inventory (id, discovery_id, payload_json, imported_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    id.to_string(),
                    serde_json::to_string(&discovery)?,
                    &imported_at,
                ],
            )?;

            let stable_identity = discovery
                .canonical_identity
                .as_deref()
                .map(str::trim)
                .filter(|identity| !identity.is_empty())
                .map(str::to_owned);

            let existing_capability_id = if let Some(identity) = stable_identity.as_deref() {
                transaction
                    .query_row(
                        "SELECT id FROM canonical_capabilities WHERE stable_identity=?1",
                        [identity],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .map(|value| parse_stored_uuid(&value, "canonical_capabilities.id"))
                    .transpose()?
            } else {
                None
            };

            let capability_id = match existing_capability_id {
                Some(capability_id) => capability_id,
                None => {
                    let capability_id = Uuid::new_v4();
                    transaction.execute(
                        "INSERT INTO canonical_capabilities
                         (id, canonical_name, stable_identity, created_at, updated_at, revision)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            capability_id.to_string(),
                            &discovery.suggested_name,
                            stable_identity.as_deref(),
                            &imported_at,
                            &imported_at,
                            Uuid::new_v4().to_string(),
                        ],
                    )?;
                    capability_id
                }
            };

            let installation_id = Uuid::new_v4();
            transaction.execute(
                "INSERT INTO installation_instances
                 (id, capability_id, source_discovery_id, kind, display_name, host,
                  installation_state, registration_state, enablement_state, runtime_state,
                  connection_state, authentication_state, aggregate_health, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'localhost', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    installation_id.to_string(),
                    capability_id.to_string(),
                    id.to_string(),
                    enum_wire_value(&discovery.suggested_type)?,
                    &discovery.suggested_name,
                    enum_wire_value(&discovery.installation_state)?,
                    enum_wire_value(&discovery.registration_state)?,
                    enum_wire_value(&discovery.enablement_state)?,
                    enum_wire_value(&discovery.runtime_state)?,
                    enum_wire_value(&discovery.connection_state)?,
                    enum_wire_value(&discovery.authentication_state)?,
                    enum_wire_value(&HealthState::Grey)?,
                    &imported_at,
                    &imported_at,
                ],
            )?;

            for hint in &discovery.health_hints {
                transaction.execute(
                    "INSERT OR IGNORE INTO health_check_definitions
                     (id, installation_id, check_type, required, expected_condition, timeout_ms,
                      stale_after_seconds, target_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        Uuid::new_v4().to_string(),
                        installation_id.to_string(),
                        enum_wire_value(&hint.check_type)?,
                        if hint.required { 1_i64 } else { 0_i64 },
                        enum_wire_value(&hint.expected_condition)?,
                        u64_to_sql(hint.timeout_ms, "health hint timeout_ms")?,
                        hint.stale_after_seconds
                            .map(|value| u64_to_sql(value, "health hint stale_after_seconds"))
                            .transpose()?,
                        serde_json::to_string(&hint.target)?,
                        &imported_at,
                        &imported_at,
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn capabilities(&self) -> Result<Vec<CanonicalCapability>, StoreError> {
        let rows = {
            let mut statement = self.connection.prepare(
                "SELECT id, canonical_name, stable_identity, created_at, updated_at, revision
                 FROM canonical_capabilities
                 ORDER BY created_at ASC, id ASC",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };

        rows.into_iter()
            .map(
                |(id, canonical_name, stable_identity, created_at, updated_at, revision)| {
                    Ok(CanonicalCapability {
                        id: parse_stored_uuid(&id, "canonical_capabilities.id")?,
                        canonical_name,
                        stable_identity,
                        created_at: parse_stored_timestamp(
                            &created_at,
                            "canonical_capabilities.created_at",
                        )?,
                        updated_at: parse_stored_timestamp(
                            &updated_at,
                            "canonical_capabilities.updated_at",
                        )?,
                        revision,
                    })
                },
            )
            .collect()
    }

    pub fn installations(&self) -> Result<Vec<InstallationInstance>, StoreError> {
        let rows = {
            let mut statement = self.connection.prepare(
                "SELECT id, capability_id, source_discovery_id, kind, display_name, host,
                        installation_state, registration_state, enablement_state, runtime_state,
                        connection_state, authentication_state, aggregate_health, created_at, updated_at
                 FROM installation_instances
                 ORDER BY created_at ASC, id ASC",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, String>(14)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };

        rows.into_iter()
            .map(
                |(
                    id,
                    capability_id,
                    source_discovery_id,
                    kind,
                    display_name,
                    host,
                    installation_state,
                    registration_state,
                    enablement_state,
                    runtime_state,
                    connection_state,
                    authentication_state,
                    aggregate_health,
                    created_at,
                    updated_at,
                )| {
                    Ok(InstallationInstance {
                        id: parse_stored_uuid(&id, "installation_instances.id")?,
                        capability_id: parse_stored_uuid(
                            &capability_id,
                            "installation_instances.capability_id",
                        )?,
                        source_discovery_id: parse_stored_uuid(
                            &source_discovery_id,
                            "installation_instances.source_discovery_id",
                        )?,
                        kind: enum_from_wire(&kind, "installation_instances.kind")?,
                        display_name,
                        host,
                        installation_state: enum_from_wire(
                            &installation_state,
                            "installation_instances.installation_state",
                        )?,
                        registration_state: enum_from_wire(
                            &registration_state,
                            "installation_instances.registration_state",
                        )?,
                        enablement_state: enum_from_wire(
                            &enablement_state,
                            "installation_instances.enablement_state",
                        )?,
                        runtime_state: enum_from_wire(
                            &runtime_state,
                            "installation_instances.runtime_state",
                        )?,
                        connection_state: enum_from_wire(
                            &connection_state,
                            "installation_instances.connection_state",
                        )?,
                        authentication_state: enum_from_wire(
                            &authentication_state,
                            "installation_instances.authentication_state",
                        )?,
                        aggregate_health: enum_from_wire(
                            &aggregate_health,
                            "installation_instances.aggregate_health",
                        )?,
                        created_at: parse_stored_timestamp(
                            &created_at,
                            "installation_instances.created_at",
                        )?,
                        updated_at: parse_stored_timestamp(
                            &updated_at,
                            "installation_instances.updated_at",
                        )?,
                    })
                },
            )
            .collect()
    }

    pub fn health_check_definitions(
        &self,
        installation_id: Uuid,
    ) -> Result<Vec<HealthCheckDefinition>, StoreError> {
        let rows = {
            let mut statement = self.connection.prepare(
                "SELECT id, installation_id, check_type, required, expected_condition, timeout_ms,
                        stale_after_seconds, target_json, created_at, updated_at
                 FROM health_check_definitions
                 WHERE installation_id=?1
                 ORDER BY created_at ASC, id ASC",
            )?;
            statement
                .query_map([installation_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };

        rows.into_iter()
            .map(
                |(
                    id,
                    stored_installation_id,
                    check_type,
                    required,
                    expected_condition,
                    timeout_ms,
                    stale_after_seconds,
                    target_json,
                    created_at,
                    updated_at,
                )| {
                    let timeout_ms = u64::try_from(timeout_ms).map_err(|_| {
                        StoreError::StorageIntegrity(
                            "health_check_definitions.timeout_ms is negative".into(),
                        )
                    })?;
                    if timeout_ms == 0 {
                        return Err(StoreError::StorageIntegrity(
                            "health_check_definitions.timeout_ms must be positive".into(),
                        ));
                    }
                    let stale_after_seconds = stale_after_seconds
                        .map(|value| {
                            u64::try_from(value).map_err(|_| {
                                StoreError::StorageIntegrity(
                                    "health_check_definitions.stale_after_seconds is negative"
                                        .into(),
                                )
                            })
                        })
                        .transpose()?;
                    let required = match required {
                        0 => false,
                        1 => true,
                        other => {
                            return Err(StoreError::StorageIntegrity(format!(
                                "invalid health_check_definitions.required value {other}"
                            )));
                        }
                    };

                    Ok(HealthCheckDefinition {
                        id: parse_stored_uuid(&id, "health_check_definitions.id")?,
                        installation_id: parse_stored_uuid(
                            &stored_installation_id,
                            "health_check_definitions.installation_id",
                        )?,
                        check_type: enum_from_wire(
                            &check_type,
                            "health_check_definitions.check_type",
                        )?,
                        required,
                        expected_condition: enum_from_wire(
                            &expected_condition,
                            "health_check_definitions.expected_condition",
                        )?,
                        timeout_ms,
                        stale_after_seconds,
                        target_metadata: serde_json::from_str::<HealthTarget>(&target_json)
                            .map_err(|error| {
                                StoreError::StorageIntegrity(format!(
                                    "invalid health_check_definitions.target_json: {error}"
                                ))
                            })?,
                        created_at: parse_stored_timestamp(
                            &created_at,
                            "health_check_definitions.created_at",
                        )?,
                        updated_at: parse_stored_timestamp(
                            &updated_at,
                            "health_check_definitions.updated_at",
                        )?,
                    })
                },
            )
            .collect()
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
            params![
                scan_id.to_string(),
                scan_scope_str(scope),
                started_at.to_rfc3339()
            ],
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
        if !matches!(
            state,
            ScanLifecycleState::Cancelled
                | ScanLifecycleState::Completed
                | ScanLifecycleState::Failed
        ) {
            return Err(StoreError::NonTerminalFinishState(state));
        }
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

    /// Upserts a scan-owned discovery. Returns `true` when the row is now
    /// pending (either freshly inserted, or an existing `pending`/`ignored`
    /// row was refreshed) -- the caller should treat this discovery as
    /// newly visible. Returns `false` when a row already existed for this
    /// fingerprint with a *different*, already-resolved review state
    /// (`imported` or `unknown`): the `ON CONFLICT ... WHERE` guard skips
    /// the update in that case, so nothing was actually persisted as
    /// pending, and the caller must not tell anything downstream (a
    /// `scan.discovery` event, the Review Queue) that this id is pending --
    /// it would silently un-resolve a decision the user already made.
    pub fn enqueue_for_scan(
        &mut self,
        scan_id: Uuid,
        discovery: &Discovery,
    ) -> Result<bool, StoreError> {
        let payload = serde_json::to_string(discovery)?;
        let changed = self.connection.execute(
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
        Ok(changed > 0)
    }
}

const M7_MIGRATION_ID: &str = "m7_health_dependency_systems_v1";

fn enum_wire_value<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(StoreError::StorageIntegrity(
            "stored enum did not serialize to a string".into(),
        )),
    }
}

fn enum_from_wire<T: serde::de::DeserializeOwned>(
    value: &str,
    field: &str,
) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(|error| {
        StoreError::StorageIntegrity(format!("invalid {field} value {value:?}: {error}"))
    })
}

fn parse_stored_uuid(value: &str, field: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(value).map_err(|error| {
        StoreError::StorageIntegrity(format!("invalid {field} UUID {value:?}: {error}"))
    })
}

fn parse_stored_timestamp(value: &str, field: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            StoreError::StorageIntegrity(format!("invalid {field} timestamp {value:?}: {error}"))
        })
}

fn u64_to_sql(value: u64, field: &str) -> Result<i64, StoreError> {
    i64::try_from(value)
        .map_err(|_| StoreError::StorageIntegrity(format!("{field} exceeds SQLite INTEGER range")))
}

fn migrate_m7_health_dependency_systems(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           id TEXT PRIMARY KEY,
           applied_at TEXT NOT NULL
         );",
    )?;

    let already_applied: i64 = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE id=?1)",
        [M7_MIGRATION_ID],
        |row| row.get(0),
    )?;
    if already_applied != 0 {
        transaction.commit()?;
        return Ok(());
    }

    transaction.execute_batch(
        "CREATE TABLE canonical_capabilities (
           id TEXT PRIMARY KEY,
           canonical_name TEXT NOT NULL,
           stable_identity TEXT UNIQUE,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           revision TEXT NOT NULL
         );
         CREATE TABLE installation_instances (
           id TEXT PRIMARY KEY,
           capability_id TEXT NOT NULL REFERENCES canonical_capabilities(id),
           source_discovery_id TEXT NOT NULL UNIQUE REFERENCES discoveries(id),
           kind TEXT NOT NULL,
           display_name TEXT NOT NULL,
           host TEXT NOT NULL,
           installation_state TEXT NOT NULL,
           registration_state TEXT NOT NULL,
           enablement_state TEXT NOT NULL,
           runtime_state TEXT NOT NULL,
           connection_state TEXT NOT NULL,
           authentication_state TEXT NOT NULL,
           aggregate_health TEXT NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE health_check_definitions (
           id TEXT PRIMARY KEY,
           installation_id TEXT NOT NULL REFERENCES installation_instances(id),
           check_type TEXT NOT NULL,
           required INTEGER NOT NULL CHECK(required IN (0,1)),
           expected_condition TEXT NOT NULL,
           timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
           stale_after_seconds INTEGER,
           target_json TEXT NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           UNIQUE(installation_id, check_type, expected_condition, target_json)
         );
         CREATE TABLE health_runs (
           id TEXT PRIMARY KEY,
           scope TEXT NOT NULL CHECK(scope IN ('installation','all')),
           state TEXT NOT NULL CHECK(state IN ('running','cancelling','cancelled','completed')),
           started_at TEXT NOT NULL,
           finished_at TEXT,
           requested_installation_count INTEGER NOT NULL,
           completed_installation_count INTEGER NOT NULL,
           revision TEXT NOT NULL
         );
         CREATE TABLE health_results (
           id TEXT PRIMARY KEY,
           run_id TEXT NOT NULL REFERENCES health_runs(id),
           installation_id TEXT NOT NULL REFERENCES installation_instances(id),
           check_definition_id TEXT NOT NULL REFERENCES health_check_definitions(id),
           check_type TEXT NOT NULL,
           required INTEGER NOT NULL CHECK(required IN (0,1)),
           state TEXT NOT NULL CHECK(state IN ('green','yellow','red','grey','blue')),
           result_code TEXT NOT NULL,
           redacted_evidence TEXT NOT NULL,
           duration_ms INTEGER NOT NULL,
           observed_at TEXT NOT NULL
         );
         CREATE TABLE dependency_relationships (
           id TEXT PRIMARY KEY,
           source_capability_id TEXT NOT NULL REFERENCES canonical_capabilities(id) ON DELETE CASCADE,
           target_capability_id TEXT NOT NULL REFERENCES canonical_capabilities(id) ON DELETE CASCADE,
           kind TEXT NOT NULL CHECK(kind IN ('requires','depends_on')),
           owner TEXT NOT NULL CHECK(owner IN ('scanner','user')),
           provenance TEXT NOT NULL,
           evidence_summary TEXT NOT NULL,
           created_at TEXT NOT NULL,
           revision TEXT NOT NULL,
           CHECK(source_capability_id <> target_capability_id),
           UNIQUE(source_capability_id, target_capability_id, kind)
         );
         CREATE INDEX idx_installations_capability ON installation_instances(capability_id);
         CREATE INDEX idx_health_definitions_installation ON health_check_definitions(installation_id);
         CREATE INDEX idx_health_results_installation_observed ON health_results(installation_id, observed_at DESC);
         CREATE INDEX idx_health_results_definition_observed ON health_results(check_definition_id, observed_at DESC);
         CREATE INDEX idx_health_runs_started ON health_runs(started_at DESC);
         CREATE INDEX idx_dependencies_source ON dependency_relationships(source_capability_id);
         CREATE INDEX idx_dependencies_target ON dependency_relationships(target_capability_id);",
    )?;

    let legacy_rows = {
        let mut statement = transaction.prepare(
            "SELECT i.discovery_id, i.payload_json, i.imported_at
             FROM inventory i
             INNER JOIN discoveries d ON d.id=i.discovery_id
             ORDER BY i.imported_at ASC, i.id ASC",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    for (source_discovery_id_raw, payload_json, imported_at_raw) in legacy_rows {
        let source_discovery_id =
            parse_stored_uuid(&source_discovery_id_raw, "inventory.discovery_id")?;
        let imported_at = parse_stored_timestamp(&imported_at_raw, "inventory.imported_at")?;
        let imported_at_text = imported_at.to_rfc3339();
        let discovery: Discovery = serde_json::from_str(&payload_json)?;
        let stable_identity = discovery
            .canonical_identity
            .as_deref()
            .map(str::trim)
            .filter(|identity| !identity.is_empty())
            .map(str::to_owned);

        let capability_id = if let Some(identity) = stable_identity.as_deref() {
            transaction
                .query_row(
                    "SELECT id FROM canonical_capabilities WHERE stable_identity=?1",
                    [identity],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| parse_stored_uuid(&value, "canonical_capabilities.id"))
                .transpose()?
        } else {
            None
        };

        let capability_id = match capability_id {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4();
                transaction.execute(
                    "INSERT INTO canonical_capabilities
                     (id, canonical_name, stable_identity, created_at, updated_at, revision)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        id.to_string(),
                        discovery.suggested_name,
                        stable_identity,
                        imported_at_text,
                        imported_at_text,
                        Uuid::new_v4().to_string(),
                    ],
                )?;
                id
            }
        };

        let installation_id = Uuid::new_v4();
        transaction.execute(
            "INSERT INTO installation_instances
             (id, capability_id, source_discovery_id, kind, display_name, host,
              installation_state, registration_state, enablement_state, runtime_state,
              connection_state, authentication_state, aggregate_health, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'localhost', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                installation_id.to_string(),
                capability_id.to_string(),
                source_discovery_id.to_string(),
                enum_wire_value(&discovery.suggested_type)?,
                discovery.suggested_name,
                enum_wire_value(&discovery.installation_state)?,
                enum_wire_value(&discovery.registration_state)?,
                enum_wire_value(&discovery.enablement_state)?,
                enum_wire_value(&discovery.runtime_state)?,
                enum_wire_value(&discovery.connection_state)?,
                enum_wire_value(&discovery.authentication_state)?,
                enum_wire_value(&HealthState::Grey)?,
                imported_at_text,
                imported_at_text,
            ],
        )?;

        for hint in discovery.health_hints {
            transaction.execute(
                "INSERT OR IGNORE INTO health_check_definitions
                 (id, installation_id, check_type, required, expected_condition, timeout_ms,
                  stale_after_seconds, target_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    Uuid::new_v4().to_string(),
                    installation_id.to_string(),
                    enum_wire_value(&hint.check_type)?,
                    if hint.required { 1_i64 } else { 0_i64 },
                    enum_wire_value(&hint.expected_condition)?,
                    u64_to_sql(hint.timeout_ms, "health hint timeout_ms")?,
                    hint.stale_after_seconds
                        .map(|value| u64_to_sql(value, "health hint stale_after_seconds"))
                        .transpose()?,
                    serde_json::to_string(&hint.target)?,
                    imported_at_text,
                    imported_at_text,
                ],
            )?;
        }
    }

    transaction.execute(
        "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
        params![M7_MIGRATION_ID, Utc::now().to_rfc3339()],
    )?;
    transaction.commit()?;
    Ok(())
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
    use crate::{
        DEFAULT_HEALTH_TIMEOUT_MS, Evidence, HealthCheckHint, HealthCheckType,
        HealthExpectedCondition, HealthState, HealthTarget,
    };
    use std::path::PathBuf;

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

        store
            .begin_scan(scan_id, ScanScope::Quick, started)
            .unwrap();
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
    fn finish_scan_rejects_non_terminal_states() {
        let mut store = test_store();
        let scan_id = Uuid::new_v4();
        let started = Utc::now();

        store
            .begin_scan(scan_id, ScanScope::Quick, started)
            .unwrap();

        let running_result = store.finish_scan(
            scan_id,
            ScanLifecycleState::Running,
            started + chrono::Duration::seconds(1),
            0,
        );
        assert!(matches!(
            running_result,
            Err(StoreError::NonTerminalFinishState(
                ScanLifecycleState::Running
            ))
        ));

        let paused_result = store.finish_scan(
            scan_id,
            ScanLifecycleState::Paused,
            started + chrono::Duration::seconds(1),
            0,
        );
        assert!(matches!(
            paused_result,
            Err(StoreError::NonTerminalFinishState(
                ScanLifecycleState::Paused
            ))
        ));

        // Row must remain untouched: still running, no finished_at.
        let row = store.scan_run_for_test(scan_id).unwrap().unwrap();
        assert_eq!(row.state, "running");
        assert!(row.finished_at.is_none());
        assert_eq!(row.failure_count, 0);
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
        assert!(store.enqueue_for_scan(scan_id, &discovery).unwrap());

        assert_eq!(
            store.discovery_scan_id_for_test(discovery.id).unwrap(),
            Some(scan_id)
        );
    }

    #[test]
    fn enqueue_for_scan_does_not_revive_an_already_resolved_discovery() {
        // A later scan rediscovering the same tool (same fingerprint) after
        // the user already imported or kept-unknown it must not silently
        // flip it back to pending -- that would un-resolve a decision the
        // user already made, and the caller must be able to tell this
        // happened so it never announces the id as newly pending.
        let mut store = test_store();
        let first_scan = Uuid::new_v4();
        let second_scan = Uuid::new_v4();
        let now = Utc::now();

        store.begin_scan(first_scan, ScanScope::Quick, now).unwrap();
        let discovery = sample_discovery();
        assert!(store.enqueue_for_scan(first_scan, &discovery).unwrap());
        store.review(discovery.id, ReviewDecision::Import).unwrap();

        store
            .begin_scan(second_scan, ScanScope::Quick, now)
            .unwrap();
        assert!(!store.enqueue_for_scan(second_scan, &discovery).unwrap());

        // The row must still show the original scan and its resolved
        // review state, not the second scan or 'pending'.
        assert_eq!(
            store.discovery_scan_id_for_test(discovery.id).unwrap(),
            Some(first_scan)
        );
        let pending_ids: Vec<_> = store.pending().unwrap().into_iter().map(|d| d.id).collect();
        assert!(!pending_ids.contains(&discovery.id));
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

    const M7_MIGRATION_ID: &str = "m7_health_dependency_systems_v1";

    fn legacy_discovery(name: &str, scanner: &str, fingerprint: &str) -> Discovery {
        let mut discovery = Discovery::unknown(name, scanner, fingerprint.into());
        discovery.review_state = ReviewState::Imported;
        discovery
    }

    fn create_pre_m7_schema(connection: &Connection) {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE scan_runs (
                   id TEXT PRIMARY KEY NOT NULL,
                   scope TEXT NOT NULL CHECK(scope IN ('quick', 'deep')),
                   state TEXT NOT NULL,
                   started_at TEXT NOT NULL,
                   finished_at TEXT,
                   failure_count INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE discoveries (
                   id TEXT PRIMARY KEY,
                   fingerprint TEXT NOT NULL UNIQUE,
                   payload_json TEXT NOT NULL,
                   review_state TEXT NOT NULL,
                   observed_at TEXT NOT NULL,
                   scan_id TEXT REFERENCES scan_runs(id)
                 );
                 CREATE TABLE inventory (
                   id TEXT PRIMARY KEY,
                   discovery_id TEXT NOT NULL UNIQUE REFERENCES discoveries(id),
                   payload_json TEXT NOT NULL,
                   imported_at TEXT NOT NULL
                 );
                 CREATE TABLE scan_errors (
                   id TEXT PRIMARY KEY NOT NULL,
                   scan_id TEXT NOT NULL REFERENCES scan_runs(id) ON DELETE CASCADE,
                   scanner_id TEXT NOT NULL,
                   code TEXT NOT NULL,
                   redacted_message TEXT NOT NULL,
                   observed_at TEXT NOT NULL
                 );
                 CREATE INDEX idx_scan_errors_scan_id ON scan_errors(scan_id);
                 CREATE INDEX idx_discoveries_scan_id ON discoveries(scan_id);",
            )
            .unwrap();
    }

    fn insert_legacy_import(connection: &Connection, discovery: &Discovery) {
        let payload = serde_json::to_string(discovery).unwrap();
        connection
            .execute(
                "INSERT INTO discoveries
                 (id, fingerprint, payload_json, review_state, observed_at, scan_id)
                 VALUES (?1, ?2, ?3, 'imported', ?4, NULL)",
                rusqlite::params![
                    discovery.id.to_string(),
                    discovery.fingerprint,
                    payload,
                    discovery.observed_at.to_rfc3339(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO inventory (id, discovery_id, payload_json, imported_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    discovery.id.to_string(),
                    serde_json::to_string(discovery).unwrap(),
                    discovery.observed_at.to_rfc3339(),
                ],
            )
            .unwrap();
    }

    fn legacy_connection_with_imported_discoveries(discoveries: &[Discovery]) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        create_pre_m7_schema(&connection);
        for discovery in discoveries {
            insert_legacy_import(&connection, discovery);
        }
        connection
    }

    fn raw_table_count_or_zero(connection: &Connection, table: &str) -> i64 {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        if exists == 0 {
            return 0;
        }
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn raw_migration_marker_count_or_zero(connection: &Connection) -> i64 {
        if raw_table_count_or_zero(connection, "schema_migrations") == 0 {
            return 0;
        }
        connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE id=?1",
                [M7_MIGRATION_ID],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn m7_migration_preserves_reviewed_discovery_linkage_and_starts_grey() {
        let discovery = legacy_discovery("Legacy CLI", "scanner-a", "fingerprint-a");
        let discovery_id = discovery.id;
        let connection = legacy_connection_with_imported_discoveries(&[discovery]);
        let store = Store::from_connection(connection).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();

        assert_eq!(capabilities.len(), 1);
        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].aggregate_health, HealthState::Grey);
        assert_eq!(installations[0].source_discovery_id, discovery_id);
        assert_eq!(installations[0].host, "localhost");
        assert_eq!(
            raw_table_count_or_zero(&store.connection, "health_results"),
            0
        );
        assert_eq!(
            raw_table_count_or_zero(&store.connection, "dependency_relationships"),
            0
        );
        assert_eq!(raw_migration_marker_count_or_zero(&store.connection), 1);
    }

    #[test]
    fn m7_migration_groups_only_matching_trusted_canonical_identity() {
        let mut first = legacy_discovery("Ollama", "scanner-a", "fingerprint-a");
        first.canonical_identity = Some("windows-path:c:\\tools\\ollama.exe".into());
        let mut second = legacy_discovery("Ollama CLI", "scanner-b", "fingerprint-b");
        second.canonical_identity = Some("windows-path:c:\\tools\\ollama.exe".into());
        let connection = legacy_connection_with_imported_discoveries(&[first, second]);
        let store = Store::from_connection(connection).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();

        assert_eq!(capabilities.len(), 1);
        assert_eq!(installations.len(), 2);
        assert_eq!(
            capabilities[0].stable_identity.as_deref(),
            Some("windows-path:c:\\tools\\ollama.exe")
        );
        assert!(
            installations
                .iter()
                .all(|item| item.capability_id == capabilities[0].id)
        );
    }

    #[test]
    fn m7_migration_does_not_group_ambiguous_legacy_rows() {
        let first = legacy_discovery("Example Tool", "scanner-a", "similar-a");
        let second = legacy_discovery("Example Tool", "scanner-b", "similar-b");
        let connection = legacy_connection_with_imported_discoveries(&[first, second]);
        let store = Store::from_connection(connection).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();

        assert_eq!(capabilities.len(), 2);
        assert_eq!(installations.len(), 2);
        assert!(
            capabilities
                .iter()
                .all(|item| item.stable_identity.is_none())
        );
        assert_ne!(
            installations[0].capability_id,
            installations[1].capability_id
        );
    }

    #[test]
    fn m7_migration_seeds_definitions_only_from_typed_hints() {
        let mut typed = legacy_discovery("Typed Tool", "scanner-a", "typed-a");
        typed.health_hints.push(HealthCheckHint {
            check_type: HealthCheckType::FileExists,
            required: true,
            expected_condition: HealthExpectedCondition::Exists,
            timeout_ms: DEFAULT_HEALTH_TIMEOUT_MS,
            stale_after_seconds: None,
            target: HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\typed.exe"),
            },
        });
        let typed_id = typed.id;

        let mut evidence_only =
            legacy_discovery("Legacy Evidence Tool", "scanner-b", "legacy-evidence");
        evidence_only.evidence.push(Evidence {
            kind: "path".into(),
            summary: r"C:\Tools\legacy.exe".into(),
        });
        let evidence_only_id = evidence_only.id;

        let connection = legacy_connection_with_imported_discoveries(&[typed, evidence_only]);
        let store = Store::from_connection(connection).unwrap();
        let installations = store.installations().unwrap();

        let typed_installation = installations
            .iter()
            .find(|item| item.source_discovery_id == typed_id)
            .unwrap();
        let legacy_installation = installations
            .iter()
            .find(|item| item.source_discovery_id == evidence_only_id)
            .unwrap();

        let definitions = store
            .health_check_definitions(typed_installation.id)
            .unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].check_type, HealthCheckType::FileExists);
        assert!(definitions[0].required);
        assert_eq!(
            definitions[0].expected_condition,
            HealthExpectedCondition::Exists
        );
        assert_eq!(
            definitions[0].target_metadata,
            HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\typed.exe"),
            }
        );
        assert!(
            store
                .health_check_definitions(legacy_installation.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn m7_migration_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("m7-idempotent.sqlite");
        let mut discovery = legacy_discovery("Stable Tool", "scanner-a", "stable-a");
        discovery.canonical_identity = Some("windows-path:c:\\tools\\stable.exe".into());
        discovery.health_hints.push(HealthCheckHint {
            check_type: HealthCheckType::FileExists,
            required: true,
            expected_condition: HealthExpectedCondition::Exists,
            timeout_ms: DEFAULT_HEALTH_TIMEOUT_MS,
            stale_after_seconds: None,
            target: HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\stable.exe"),
            },
        });

        {
            let connection = Connection::open(&database).unwrap();
            create_pre_m7_schema(&connection);
            insert_legacy_import(&connection, &discovery);
        }

        {
            let store = Store::open(&database).unwrap();
            assert_eq!(store.capabilities().unwrap().len(), 1);
            assert_eq!(store.installations().unwrap().len(), 1);
            assert_eq!(
                store
                    .health_check_definitions(store.installations().unwrap()[0].id)
                    .unwrap()
                    .len(),
                1
            );
        }

        let store = Store::open(&database).unwrap();
        assert_eq!(store.capabilities().unwrap().len(), 1);
        assert_eq!(store.installations().unwrap().len(), 1);
        assert_eq!(
            store
                .health_check_definitions(store.installations().unwrap()[0].id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(raw_migration_marker_count_or_zero(&store.connection), 1);
    }

    #[test]
    fn m7_migration_failure_rolls_back_everything() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("m7-rollback.sqlite");
        {
            let connection = Connection::open(&database).unwrap();
            create_pre_m7_schema(&connection);
            let valid = legacy_discovery("Valid Tool", "scanner-a", "valid-a");
            insert_legacy_import(&connection, &valid);

            let malformed_id = Uuid::new_v4();
            connection
                .execute(
                    "INSERT INTO discoveries
                     (id, fingerprint, payload_json, review_state, observed_at, scan_id)
                     VALUES (?1, ?2, ?3, 'imported', ?4, NULL)",
                    rusqlite::params![
                        malformed_id.to_string(),
                        "malformed-b",
                        "{not valid discovery json",
                        Utc::now().to_rfc3339(),
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO inventory (id, discovery_id, payload_json, imported_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        Uuid::new_v4().to_string(),
                        malformed_id.to_string(),
                        "{not valid discovery json",
                        Utc::now().to_rfc3339(),
                    ],
                )
                .unwrap();
        }

        let result = Store::open(&database);
        assert!(result.is_err());
        drop(result);

        let raw = Connection::open(&database).unwrap();
        assert_eq!(raw_migration_marker_count_or_zero(&raw), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "canonical_capabilities"), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "installation_instances"), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "health_check_definitions"), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "health_runs"), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "health_results"), 0);
        assert_eq!(raw_table_count_or_zero(&raw, "dependency_relationships"), 0);
    }

    #[test]
    fn m7_review_import_creates_capability_installation_and_typed_definitions() {
        let mut store = Store::in_memory().unwrap();
        let mut discovery = Discovery::unknown("Reviewed CLI", "scanner-a", "review-a".into());
        discovery.canonical_identity = Some("windows-path:c:\\tools\\reviewed.exe".into());
        discovery.health_hints.push(HealthCheckHint {
            check_type: HealthCheckType::FileExists,
            required: true,
            expected_condition: HealthExpectedCondition::Exists,
            timeout_ms: DEFAULT_HEALTH_TIMEOUT_MS,
            stale_after_seconds: None,
            target: HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\reviewed.exe"),
            },
        });
        let discovery_id = discovery.id;

        store.enqueue(&discovery).unwrap();
        store.review(discovery.id, ReviewDecision::Import).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();
        assert_eq!(capabilities.len(), 1);
        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].source_discovery_id, discovery_id);
        assert_eq!(installations[0].aggregate_health, HealthState::Grey);
        assert_eq!(installations[0].capability_id, capabilities[0].id);

        let definitions = store.health_check_definitions(installations[0].id).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].check_type, HealthCheckType::FileExists);
        assert!(definitions[0].required);
        assert_eq!(
            definitions[0].target_metadata,
            HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\reviewed.exe"),
            }
        );
    }

    #[test]
    fn m7_review_import_reuses_only_exact_stable_identity() {
        let mut store = Store::in_memory().unwrap();

        let mut first = Discovery::unknown("Shared Tool", "scanner-a", "review-shared-a".into());
        first.canonical_identity = Some("windows-path:c:\\tools\\shared.exe".into());
        store.enqueue(&first).unwrap();
        store.review(first.id, ReviewDecision::Import).unwrap();

        let mut second =
            Discovery::unknown("Shared Tool CLI", "scanner-b", "review-shared-b".into());
        second.canonical_identity = Some("windows-path:c:\\tools\\shared.exe".into());
        store.enqueue(&second).unwrap();
        store.review(second.id, ReviewDecision::Import).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();
        assert_eq!(capabilities.len(), 1);
        assert_eq!(installations.len(), 2);
        assert!(
            installations
                .iter()
                .all(|item| item.capability_id == capabilities[0].id)
        );

        let third = Discovery::unknown("Shared Tool", "scanner-c", "review-ambiguous-c".into());
        store.enqueue(&third).unwrap();
        store.review(third.id, ReviewDecision::Import).unwrap();

        let capabilities = store.capabilities().unwrap();
        let installations = store.installations().unwrap();
        assert_eq!(capabilities.len(), 2);
        assert_eq!(installations.len(), 3);
        assert_eq!(
            capabilities
                .iter()
                .filter(|item| item.stable_identity.as_deref()
                    == Some("windows-path:c:\\tools\\shared.exe"))
                .count(),
            1
        );
        assert_eq!(
            capabilities
                .iter()
                .filter(|item| item.stable_identity.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn m7_review_import_without_health_hints_stays_grey_without_definitions() {
        let mut store = Store::in_memory().unwrap();
        let mut discovery =
            Discovery::unknown("Evidence Only", "scanner-a", "review-evidence".into());
        discovery.evidence.push(Evidence {
            kind: "path".into(),
            summary: r"C:\Tools\evidence-only.exe".into(),
        });
        let discovery_id = discovery.id;

        store.enqueue(&discovery).unwrap();
        store.review(discovery.id, ReviewDecision::Import).unwrap();

        let installations = store.installations().unwrap();
        let installation = installations
            .iter()
            .find(|item| item.source_discovery_id == discovery_id)
            .unwrap();
        assert_eq!(installation.aggregate_health, HealthState::Grey);
        assert!(
            store
                .health_check_definitions(installation.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn m7_review_ignore_and_unknown_do_not_create_inventory_entities() {
        let mut store = Store::in_memory().unwrap();

        let ignored = Discovery::unknown("Ignored Tool", "scanner-a", "review-ignore".into());
        store.enqueue(&ignored).unwrap();
        store.review(ignored.id, ReviewDecision::Ignore).unwrap();

        let unknown = Discovery::unknown("Unknown Tool", "scanner-b", "review-unknown".into());
        store.enqueue(&unknown).unwrap();
        store
            .review(unknown.id, ReviewDecision::KeepUnknown)
            .unwrap();

        assert!(store.capabilities().unwrap().is_empty());
        assert!(store.installations().unwrap().is_empty());
        assert!(store.inventory().unwrap().is_empty());
    }
}
