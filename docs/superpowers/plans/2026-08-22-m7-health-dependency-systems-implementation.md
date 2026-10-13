# Milestone 7 Health and Dependency Systems Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Milestone 7 by replacing the legacy reviewed-inventory projection with canonical capabilities and concrete installation instances, adding explicit on-demand typed local health checks with durable history and cancellation, and adding reviewed machine-derived plus user-authored dependency relationships without weakening the review-first, local-only security model.

**Architecture:** Preserve M6 scan orchestration as a separate subsystem. Extend reviewed discoveries with backward-compatible typed machine metadata so new native observations can seed generic M7 health definitions without reparsing display evidence, migrate legacy imported inventory transactionally into canonical capabilities plus installation instances, and keep ambiguous legacy identity separate. Add a core health evaluator/probe layer and an independent health-run coordinator, then expose them through dedicated Tauri health commands with bounded concurrency, cancellation, revision protection and persist-before-notify semantics. Dependencies live between canonical capabilities and are persisted independently of health, with scanner-owned relationships created only through the review gate and user-owned relationships protected from scanner overwrite.

**Tech Stack:** Rust 1.96, Tokio 1.53.1, rusqlite 0.40.1, Tauri 2.11.5, React 19.2.8, TypeScript 7.0.2, Vitest 4.1.10, Playwright 1.62.1 and pnpm 10.33.2.

**Spec:** `docs/superpowers/specs/2026-08-22-m7-health-dependency-systems-design.md`

## Global Constraints

- M7 remains separate from M6 scan orchestration, M8 operational controls and M9 product-specific adapter behavior.
- Preserve all completed M6 Quick Scan and Deep Scan lifecycle, persistence, cancellation, pause/resume, review-gate and frontend behaviors.
- Health checks run only after an explicit user action.
- Supported health triggers are one reviewed installation and Check All reviewed installations.
- Never run health checks automatically on launch, import, navigation, scan settlement or application idle time.
- Imported and migrated installation instances begin `Grey / Not checked`.
- Never infer Green merely because a discovery or installation exists.
- Generic M7 check types are exactly `path_exists`, `file_exists`, `config_present`, `config_parses`, `process_running`, `windows_service_state` and `loopback_port_reachable`.
- Product-specific health rules remain deferred to M9 adapter work.
- M7 `config_parses` supports JSON only because JSON is the only safely supported configuration parser currently present in the Rust core.
- Do not add YAML or TOML parsing merely to broaden M7.
- Health belongs to `InstallationInstance`.
- Dependencies belong to `CanonicalCapability`.
- Automatic capability grouping is allowed only when a trusted strong deterministic canonical identity exists.
- `Discovery.fingerprint` is scanner-specific discovery identity and must not be promoted into cross-scanner canonical identity by migration.
- Weak name similarity, path similarity, executable similarity and other heuristics must never silently merge capabilities.
- Ambiguous legacy rows remain separate canonical capabilities.
- Every legacy imported row retains its reviewed `source_discovery_id` linkage and original persisted discovery/evidence record.
- Legacy migration initializes aggregate health to Grey.
- Legacy migration creates no health history.
- Legacy migration invents no dependency relationships.
- Legacy migration must be one SQLite transaction and roll back completely on failure.
- New native discoveries may carry typed generic M7 health hints.
- Never recover executable health targets by parsing `Evidence.summary`.
- Existing legacy rows that lack typed health metadata remain Grey rather than receiving fabricated check definitions.
- Python/config discoveries remain Grey unless they later carry a safely typed generic target; do not expand the Python protocol into adapter-specific M9 health behavior in M7.
- New native file/path discoveries may seed typed file/path checks directly from their structured scanner values.
- Process discoveries may seed `process_running` only when a sufficiently strong exact process target is available; name-only observations must not be upgraded into a stronger target.
- Windows service discoveries may seed an exact service-name `windows_service_state` check.
- TCP discoveries may seed `loopback_port_reachable` only for an actual loopback address.
- Non-loopback health networking is forbidden.
- Health probes are read-only, local, bounded and non-destructive.
- Health probes never start, stop or restart processes or services.
- Health probes never install, repair, update or uninstall software.
- Health probes never mutate configuration.
- Health probes never inspect or persist secrets.
- Health probes never hydrate cloud-only content.
- Health probes never make paid API calls.
- Health probes never execute arbitrary product-specific parsers or commands.
- Filesystem, configuration, process, service and socket probe I/O must run without holding the SQLite `Store` lock.
- Every individual check has a hard configured timeout.
- Check All executes at most 4 installation workers concurrently.
- A slow or failed installation must not prevent unrelated installations from progressing.
- Only one active health run may own a given installation at a time.
- A Check All run must not duplicate work already owned by another active run.
- Health cancellation is cooperative and must work for both one-installation and Check All runs.
- Once cancellation wins, no new checks start.
- Already completed and persisted health results remain preserved after cancellation.
- Untouched checks retain their prior applicable results.
- Cancellation observations may be persisted for audit but must not displace the latest previously completed applicable result when calculating current health.
- Every completed health check produces an append-only `HealthResult`.
- Historical results are never rewritten to represent a newer state.
- The latest applicable result for each configured definition contributes to current health.
- Generic M7 checks default to no automatic expiry.
- `stale_after_seconds` is optional and evaluated per check.
- No universal 24-hour, 7-day or other global health TTL is allowed.
- Staleness alone must never produce Red.
- Missing or insufficient required evidence remains Grey.
- Green requires every configured required check to have a current applicable passing result.
- Required failure produces Red.
- Required setup/user-action state produces Blue.
- Optional failures, optional warnings and otherwise-passing stale evidence contribute Yellow rather than Red.
- Aggregate precedence is deterministic: Red > Blue > Grey > Yellow > Green.
- Persist each health result before notifying the frontend that the check completed.
- Recompute and persist the installation's non-stale aggregate after each applicable result before emitting `health.check_completed` or `health.installation_updated`.
- Backend health projections must re-evaluate TTL staleness at read time so health can become effectively Yellow without a background task.
- Frontend health events are notifications, never the source of truth.
- After terminal health events, the frontend refreshes authoritative backend projections.
- Health result evidence passes through the existing Rust redaction boundary before persistence or IPC.
- Raw SQLite error strings, secrets and unsafe evidence must not cross the Tauri IPC boundary.
- Negative observed conditions such as missing file, stopped process or unreachable loopback port are health data, not command transport failures.
- Dependency kinds are exactly `requires` and `depends_on`.
- Dependency owner is exactly `scanner` or `user`.
- Scanner-owned dependencies originate only from reviewed machine evidence.
- A pending discovery may carry a typed dependency proposal, but it becomes active only when that discovery is imported.
- Scanner dependency materialization requires an allowed kind, deterministic source capability, deterministic target capability, non-self reference and no logical duplicate.
- Unresolved or ambiguous machine dependency targets create no relationship.
- User-owned dependency relationships must never be overwritten or silently deleted by scanner activity.
- Scanner-owned dependency relationships are read-only in the manual editor.
- Users may create and delete only user-owned dependency relationships.
- Reject dependency self-reference.
- Reject logical duplicate dependencies.
- Reject missing source or target capabilities.
- Dependency mutations use opaque revision protection and return `conflict` on a stale revision without mutation.
- Health-run mutations extend the same opaque revision-token pattern without sharing mutable lifecycle state with M6 scans.
- Closing the Tauri main window cancels all active health tokens as well as all active scan tokens.
- Health and scan registries remain independent.
- Keep generated Tauri schemas generated; do not hand-edit `apps/desktop/src-tauri/gen/schemas/*`.
- Do not introduce a paid service, remote dependency, telemetry or background networking.
- Do not add a frontend graph library for M7; the dependency graph is lightweight and local.
- Preserve local-only and zero-telemetry copy.
- Follow TDD for every implementation task.
- Run the focused RED test before writing the implementation for that task.
- Do not weaken an existing M6 test to make M7 pass.
- Do not claim a gate passed unless the command was actually run and exited successfully.

## File Responsibility Map

### Rust core

- Modify `engine/rust/control-center-core/src/model.rs`
  - Add canonical capability and installation-instance domain types.
  - Add the five-state health enum.
  - Add typed health-check definitions, targets, expectations, run/result types and dependency relationship types.
  - Add backward-compatible optional discovery canonical identity, generic health hints and machine dependency proposals using `#[serde(default)]`.
  - Keep existing `Discovery` payloads deserializable.

- Create `engine/rust/control-center-core/src/health.rs`
  - Own deterministic health aggregation.
  - Own staleness evaluation.
  - Own validation of typed check definitions.
  - Execute the seven generic read-only M7 probes.
  - Reuse existing Windows process and service source abstractions instead of duplicating enumeration.
  - Enforce JSON-only config parsing, bounded config reads, loopback-only sockets and redacted result evidence.

- Create `engine/rust/control-center-core/src/health_control.rs`
  - Own health-run scope and lifecycle contracts.
  - Own bounded installation-level orchestration.
  - Enforce maximum Check All concurrency of 4.
  - Enforce per-check timeout and cancellation-before-new-work semantics.
  - Emit typed health lifecycle/check events independently of `scan_control.rs`.

- Modify `engine/rust/control-center-core/src/windows.rs`
  - Populate typed generic health hints from native path, process, service, uninstall-location and loopback observations where the scanner already has structured target data.
  - Do not parse evidence summaries.
  - Do not add product-specific expectations.

- Modify `engine/rust/control-center-core/src/deep_scan.rs`
  - Attach a typed file target to relevant discovered files.
  - Preserve existing traversal, redaction and M6 performance behavior.

- Modify `engine/rust/control-center-core/src/scan.rs`
  - Attach typed file targets to Quick filesystem discoveries where the path is already structured.
  - Preserve scanner orchestration and event behavior.

- Modify `engine/rust/control-center-core/src/python_supervisor.rs`
  - Preserve backward-compatible discovery deserialization with the new optional metadata fields.
  - Do not add M9 adapter-specific health behavior.

- Modify `engine/rust/control-center-core/src/storage.rs`
  - Add the transactional M7 schema migration.
  - Preserve legacy discovery and evidence linkage.
  - Replace authoritative reviewed-inventory reads with capability/installation projections.
  - Change import review to create canonical capability plus installation atomically.
  - Seed generic health definitions only from typed reviewed hints.
  - Persist health runs/results and aggregate state.
  - Expose current health projections and compact history.
  - Persist dependency relationships and enforce ownership/duplicate rules.
  - Materialize unambiguous scanner-owned proposals only through import review.
  - Keep legacy `inventory` data non-authoritative after migration rather than reparsing its evidence.

- Modify `engine/rust/control-center-core/src/lib.rs`
  - Export M7 health and domain contracts without changing the public M6 scan contract.

### Tauri desktop

- Create `apps/desktop/src-tauri/src/health_commands.rs`
  - Own the active health registry.
  - Own health workspace and per-run revisions.
  - Expose `list_health_summary`, `list_installation_health`, `run_health_checks`, `run_all_health_checks` and `cancel_health_run`.
  - Bridge core health events to one `health:event` channel.
  - Persist before emitting frontend notifications.
  - Keep `Store` locks short-lived and outside probe I/O.
  - Map internal failures to stable redacted command errors.

- Create `apps/desktop/src-tauri/src/dependency_commands.rs`
  - Expose `list_dependencies`, `create_user_dependency` and `delete_user_dependency`.
  - Own dependency mutation revision validation.
  - Reject invalid, duplicate, self-referential and scanner-owned deletion requests with stable codes.

- Modify `apps/desktop/src-tauri/src/scan_commands.rs`
  - Move only the shared `AppState` responsibility out of scan-specific ownership if needed by the new command modules.
  - Preserve every existing scan registry and scan command behavior.

- Modify `apps/desktop/src-tauri/src/lib.rs`
  - Hold shared application state for Store, scan registry, health registry and dependency revision.
  - Extend bootstrap projections for canonical capabilities/installations and revisions.
  - Register the M7 commands.
  - Cancel active health work on window close.
  - Replace raw `StoreError::to_string()` IPC leakage with stable redacted storage errors where M7 touches the boundary.

### React frontend

- Modify `apps/frontend/src/model.ts`
  - Replace legacy reviewed `Discovery[]` inventory projection with canonical capability and installation types.
  - Add health summary/detail/run/event types.
  - Add dependency relationship and revision types.

- Modify `apps/frontend/src/api.ts`
  - Add M7 invoke wrappers and the `health:event` subscription.
  - Keep browser/demo behavior synthetic and filesystem-free.
  - Preserve existing M6 scan API behavior.

- Create `apps/frontend/src/useHealth.ts`
  - Own frontend health orchestration state across page navigation.
  - Attach the health event listener before invoking a run so fast events are not lost.
  - Refresh authoritative projections after terminal events.
  - Provide a timer-driven synthetic browser health run for E2E without touching the host machine.

- Create `apps/frontend/src/HealthPage.tsx`
  - Render reviewed installation health rows, five states, per-item Run checks, Check All, progress, Cancel, detail and compact history.
  - Render Grey / Not checked for installations with no applicable completed required checks.

- Create `apps/frontend/src/DependenciesPage.tsx`
  - Render the accessible relationship list as the primary view.
  - Render a lightweight dependency graph as a secondary view without a graph-library dependency.
  - Provide source, relationship-kind and target controls for user-owned relationships.
  - Never expose a delete control for scanner-owned relationships.

- Modify `apps/frontend/src/App.tsx`
  - Wire Health and Dependencies pages instead of `EmptyPage`.
  - Render installation instances in Inventory with aggregate health.
  - Replace hard-coded Overview health values with backend-derived installation counts.
  - Distinguish canonical capability count, installation-instance count and pending-discovery count.
  - Refresh authoritative bootstrap state after import rather than optimistically treating a `Discovery` as an installation.
  - Preserve scan state while navigating Health, Inventory and Dependencies.

- Modify `apps/frontend/src/styles.css`
  - Add compact five-state health, progress/detail/history and dependency-list/graph styling.
  - Preserve the existing local command-center visual language.

- Modify `apps/frontend/src/App.test.tsx`
  - Preserve browser review/scan tests.
  - Add synthetic health navigation, cancellation and completed-result preservation coverage.

- Modify `apps/frontend/src/App.desktop.test.tsx`
  - Extend API mocks for M7.
  - Test Health page states, explicit run triggers, Check All, cancellation and refresh behavior.
  - Test Inventory and Overview real health projections.
  - Test dependency direction, ownership, create/delete, duplicate/self-reference errors and absence of scanner delete controls.

- Modify `apps/frontend/e2e/scan-orchestration.spec.ts`
  - Preserve the existing M6 scenarios.
  - Add the required slow-health-run navigation/cancellation scenario, or split that scenario into the dedicated file below if keeping scan E2E focused is clearer.

- Create `apps/frontend/e2e/health-dependencies.spec.ts`
  - Exercise a deliberately slow synthetic health run in a real browser.
  - Navigate among Health, Inventory and Dependencies while the run remains active.
  - Cancel the run.
  - Verify completed persisted synthetic results remain visible and untouched checks retain prior state.
  - Prove navigation itself never starts a health run.

---

### Task 1: M7 domain model and typed reviewed machine metadata

**Files:**
- Modify: `engine/rust/control-center-core/src/model.rs`
- Modify: `engine/rust/control-center-core/src/windows.rs`
- Modify: `engine/rust/control-center-core/src/deep_scan.rs`
- Modify: `engine/rust/control-center-core/src/scan.rs`
- Modify: `engine/rust/control-center-core/src/python_supervisor.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline `#[cfg(test)]` modules in the files above

**Interfaces:**

- Consumes existing `ToolKind`, `ObservedState`, `Confidence`, `ReviewState`, `Evidence` and `Discovery`.
- Consumes existing `windows_path_key(&Path) -> String`.
- Consumes scanner-owned structured values such as executable `PathBuf`, exact service name and TCP address/port.
- Produces M7 health, canonical inventory and dependency domain contracts in `model.rs`.

- Produces `HealthState { Green, Yellow, Red, Grey, Blue }`.
- Produces `HealthCheckType { PathExists, FileExists, ConfigPresent, ConfigParses, ProcessRunning, WindowsServiceState, LoopbackPortReachable }`.
- Produces `ConfigFormat { Json }`.
- Produces `HealthExpectedCondition { Exists, Parses, Running, Reachable }`.
- Produces tagged `HealthTarget`: `Path { path: PathBuf }`, `Config { path: PathBuf, format: ConfigFormat }`, `ProcessExecutable { path: PathBuf }`, `WindowsService { service_name: String }`, `LoopbackSocket { host: String, port: u16 }`.
- Produces `HealthCheckHint { check_type, required, expected_condition, timeout_ms, stale_after_seconds, target }`.
- Produces `DependencyKind { Requires, DependsOn }` and `DependencyProposal { kind, target_stable_identity, evidence_summary }`.
- Produces `CanonicalCapability { id, canonical_name, stable_identity, created_at, updated_at, revision }`.
- Produces `InstallationInstance { id, capability_id, source_discovery_id, kind, display_name, host, installation_state, registration_state, enablement_state, runtime_state, connection_state, authentication_state, aggregate_health, created_at, updated_at }`.
- Produces `HealthCheckDefinition { id, installation_id, check_type, required, expected_condition, timeout_ms, stale_after_seconds, target_metadata, created_at, updated_at }`.
- Produces `HealthRunScope { Installation, All }`, `HealthRunState { Running, Cancelling, Cancelled, Completed }`, `HealthRun` and append-only `HealthResult`.
- Produces `DependencyOwner { Scanner, User }` and `DependencyRelationship { id, source_capability_id, target_capability_id, kind, owner, provenance, evidence_summary, created_at, revision }`.

Extend `Discovery` with:

```rust
#[serde(default)]
pub canonical_identity: Option<String>,

#[serde(default)]
pub health_hints: Vec<HealthCheckHint>,

#[serde(default)]
pub dependency_proposals: Vec<DependencyProposal>,
```

`Discovery::unknown(...)` initializes them to `None`, `Vec::new()` and `Vec::new()`.

Add `pub const DEFAULT_HEALTH_TIMEOUT_MS: u64 = 2_000;`. Generic generated hints are required, have `stale_after_seconds: None`, and identical `(check_type, target)` hints are deduplicated.

Canonical identity strings are opaque internal identities, not display names. M7 permits only these deterministic forms:

```text
windows-path:<normalized windows_path_key>
windows-service:<lowercase exact service name>
loopback-tcp:<normalized loopback host>:<port>
```

A scanner that cannot prove one of these identities leaves `canonical_identity = None`.

Typed native metadata rules:

| Scanner evidence | Canonical identity | Generic health hint |
| --- | --- | --- |
| known-location executable/script | `windows-path:<key>` | required `file_exists` |
| PATH executable | `windows-path:<key>` | required `file_exists` |
| Quick filesystem relevant file | `windows-path:<key>` | required `file_exists` |
| Deep Scan relevant file | `windows-path:<key>` | required `file_exists` |
| process with exact executable path | `windows-path:<key>` | required `process_running` against exact executable path |
| process with name only | none | none |
| Windows service | `windows-service:<service>` | required `windows_service_state`, expected Running |
| loopback TCP listener | `loopback-tcp:<host>:<port>` | required `loopback_port_reachable` |
| non-loopback TCP listener | none | none |
| uninstall record with structured install directory | none | required `path_exists` for that exact directory only |
| Python/config discovery | none unless future typed generic metadata exists | none in M7 |
| shortcut | shortcut path identity only | required `file_exists` for the shortcut itself |

Never parse `Evidence.summary` to create a canonical identity or health target. An uninstall install directory is not promoted into canonical identity. Name-only process observations receive no process health check. Non-loopback TCP observations receive no reachability check.

- [ ] **Step 1: Add failing backward-compatibility and wire-value tests**

In `model.rs`, add:

```rust
#[test]
fn legacy_discovery_payload_defaults_m7_metadata() {
    let payload = r#"{
        "id":"00000000-0000-0000-0000-000000000001",
        "fingerprint":"legacy",
        "suggested_name":"Legacy Tool",
        "suggested_type":"cli",
        "source_scanner":"windows.path",
        "confidence":"medium",
        "evidence":[],
        "observed_at":"2026-08-22T00:00:00Z",
        "review_state":"pending",
        "installation_state":"detected",
        "registration_state":"unknown",
        "enablement_state":"unknown",
        "runtime_state":"unknown",
        "connection_state":"unknown",
        "authentication_state":"unknown",
        "health_state":"unknown"
    }"#;

    let discovery: Discovery = serde_json::from_str(payload).unwrap();

    assert_eq!(discovery.canonical_identity, None);
    assert!(discovery.health_hints.is_empty());
    assert!(discovery.dependency_proposals.is_empty());
}

#[test]
fn m7_wire_values_are_stable_snake_case() {
    assert_eq!(serde_json::to_string(&HealthState::Grey).unwrap(), r#""grey""#);
    assert_eq!(
        serde_json::to_string(&HealthCheckType::LoopbackPortReachable).unwrap(),
        r#""loopback_port_reachable""#
    );
    assert_eq!(
        serde_json::to_string(&DependencyKind::DependsOn).unwrap(),
        r#""depends_on""#
    );
    assert_eq!(
        serde_json::to_string(&DependencyOwner::Scanner).unwrap(),
        r#""scanner""#
    );
}
```

- [ ] **Step 2: Run the focused model test and verify RED**

Run: `cargo test -p control-center-core legacy_discovery_payload_defaults_m7_metadata --locked`

Expected: compilation fails because the M7 fields and types do not exist yet.

- [ ] **Step 3: Implement the M7 domain contracts and Discovery defaults**

Implement the exact types and fields defined above in `model.rs`. Preserve the existing `Discovery.health_state: ObservedState` field because pending discovery payloads, M6 scanner tests and pre-M7 stored JSON still depend on it. It is not authoritative for reviewed installation health after M7. Do not rename or remove existing M6 model fields.

- [ ] **Step 4: Run the focused model tests and verify GREEN**

Run: `cargo test -p control-center-core legacy_discovery_payload_defaults_m7_metadata --locked`

Expected: PASS.

Run: `cargo test -p control-center-core m7_wire_values_are_stable_snake_case --locked`

Expected: PASS.

- [ ] **Step 5: Add failing typed-target scanner tests**

Add focused tests beside the existing scanner tests. Name the new tests with a shared `typed_` prefix.

For PATH and known-location discoveries, use existing temporary-directory fixtures and assert that a discovered executable carries `canonical_identity` beginning with `windows-path:` plus a required `HealthCheckType::FileExists` hint whose target is the exact structured path.

For process discovery, use a fake `ProcessSource` containing one record with `executable_path: Some(...)` and one name-only record. Assert the exact-path process receives `windows-path:<key>` plus a required `process_running` hint against `HealthTarget::ProcessExecutable`, while the name-only process receives neither canonical identity nor process health hint.

For Windows service discovery, use a fake `ServiceSource` and assert the exact service name is preserved in `HealthTarget::WindowsService`, expected condition is `Running`, and canonical identity is `windows-service:<lowercase service name>`.

For TCP discovery, provide one loopback record and one non-loopback record through a fake `TcpEndpointSource`. Assert only the loopback endpoint receives `loopback_port_reachable` and `loopback-tcp:<host>:<port>`.

For uninstall registry discovery, assert a structured `install_location` may produce required `path_exists`, but does not become canonical identity.

For Quick filesystem and Deep Scan, assert a relevant discovered file carries a typed required `file_exists` hint while existing evidence summaries and scan event semantics remain unchanged.

For Python/config translation, assert legacy Python responses without M7 metadata still translate successfully and do not infer health targets from evidence strings.

- [ ] **Step 6: Run the typed scanner tests and verify RED**

Run: `cargo test -p control-center-core typed_ --locked`

Expected: FAIL because scanners still populate only legacy display evidence.

- [ ] **Step 7: Implement deterministic native metadata helpers**

In `windows.rs`, add these exact helpers:

```rust
fn canonical_windows_path_identity(path: &Path) -> String {
    format!("windows-path:{}", windows_path_key(path))
}

fn canonical_windows_service_identity(service_name: &str) -> String {
    format!("windows-service:{}", service_name.trim().to_ascii_lowercase())
}

fn canonical_loopback_identity(host: &str, port: u16) -> Option<String> {
    let address: std::net::IpAddr = host.trim().parse().ok()?;
    address
        .is_loopback()
        .then(|| format!("loopback-tcp:{address}:{port}"))
}
```

Populate hints only from scanner-owned structured fields. Never parse `Evidence.summary`. Use `DEFAULT_HEALTH_TIMEOUT_MS`, `required: true`, and `stale_after_seconds: None`.

- [ ] **Step 8: Add typed file metadata to Quick and Deep filesystem discoveries**

Where Quick filesystem and Deep Scan already own the concrete path, set a deterministic Windows-path identity and a required `file_exists` hint against that exact path. Keep this metadata construction portable for non-Windows unit tests and do not add Windows FFI to Deep Scan.

- [ ] **Step 9: Run typed scanner tests and verify GREEN**

Run: `cargo test -p control-center-core typed_ --locked`

Expected: all new typed metadata tests PASS.

- [ ] **Step 10: Run M6 regression tests**

Run: `cargo test -p control-center-core windows --locked`

Expected: PASS.

Run: `cargo test -p control-center-core scan --locked`

Expected: PASS.

Run: `cargo test -p control-center-core deep_scan --locked`

Expected: PASS.

Run: `cargo test -p control-center-core python_supervisor --locked`

Expected: PASS.

- [ ] **Step 11: Preserve the existing model export boundary**

`lib.rs` already exports `pub use model::*;`. Keep that export. Do not create empty health modules in Task 1. Later tasks add `health.rs` and `health_control.rs` only when they contain tested implementation.

- [ ] **Step 12: Commit Task 1**

Run: `git add engine/rust/control-center-core/src/model.rs engine/rust/control-center-core/src/windows.rs engine/rust/control-center-core/src/deep_scan.rs engine/rust/control-center-core/src/scan.rs engine/rust/control-center-core/src/python_supervisor.rs engine/rust/control-center-core/src/lib.rs`

Run: `git commit -m "feat: add typed M7 inventory metadata"`

Expected: one focused commit containing M7 domain contracts and typed discovery metadata only.

---

### Task 2: Transactional canonical inventory migration and M7 persistence schema

**Files:**
- Modify: `engine/rust/control-center-core/src/storage.rs`
- Test: inline `#[cfg(test)]` module in `storage.rs`

**Interfaces:**

- Consumes `Discovery.canonical_identity`, `Discovery.health_hints`, reviewed legacy `inventory.discovery_id` linkage and the M7 model types from Task 1.
- Produces authoritative `canonical_capabilities`, `installation_instances`, `health_check_definitions`, `health_runs`, `health_results` and `dependency_relationships` tables.
- Produces `Store::capabilities() -> Result<Vec<CanonicalCapability>, StoreError>`.
- Produces `Store::installations() -> Result<Vec<InstallationInstance>, StoreError>`.
- Produces `Store::health_check_definitions(installation_id: Uuid) -> Result<Vec<HealthCheckDefinition>, StoreError>`.
- Keeps the legacy `inventory` table readable for migration provenance, but it is no longer the authoritative reviewed-inventory projection after successful M7 migration.

Add a migration marker table:

```sql
CREATE TABLE IF NOT EXISTS schema_migrations (
    id TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

Use migration id `m7_health_dependency_systems_v1`. `Store::from_connection` must create the pre-existing base tables first, then run the M7 migration before returning the Store. Change its local connection binding to mutable if required by `rusqlite::Connection::transaction()`.

The M7 schema uses TEXT UUIDs and RFC3339 timestamps consistently with existing storage. Add foreign keys and indexes needed for installation lookup, latest health results and dependency traversal.

---

Create the M7 tables inside the migration transaction with this logical schema:

```sql
CREATE TABLE canonical_capabilities (
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
```

Add indexes:

```sql
CREATE INDEX idx_installations_capability ON installation_instances(capability_id);
CREATE INDEX idx_health_definitions_installation ON health_check_definitions(installation_id);
CREATE INDEX idx_health_results_installation_observed ON health_results(installation_id, observed_at DESC);
CREATE INDEX idx_health_results_definition_observed ON health_results(check_definition_id, observed_at DESC);
CREATE INDEX idx_health_runs_started ON health_runs(started_at DESC);
CREATE INDEX idx_dependencies_source ON dependency_relationships(source_capability_id);
CREATE INDEX idx_dependencies_target ON dependency_relationships(target_capability_id);
```

Serialize enum columns with their stable snake_case wire values. Serialize `HealthTarget` into `target_json` with `serde_json`. Parse every stored enum/UUID/timestamp through typed conversion on read; invalid persisted values return `StoreError::InvalidData` or a dedicated storage-integrity variant rather than silently defaulting.

Migration algorithm:
1. Begin one SQLite transaction.
2. Create `schema_migrations` and all M7 tables/indexes inside that transaction if the marker is absent.
3. Read every existing legacy `inventory` row joined to its `discoveries` row so `discovery_id`, serialized `Discovery` and evidence linkage remain available.
4. Deserialize the Discovery using Task 1 backward-compatible defaults.
5. Resolve a canonical capability only by non-empty `Discovery.canonical_identity`. If the same trusted identity was already created in this migration, reuse that capability. Otherwise create a new capability. Rows with `canonical_identity = None` always create separate capabilities, even if names or fingerprints resemble one another.
6. Create exactly one `InstallationInstance` per legacy inventory row with `source_discovery_id` preserved and `aggregate_health = Grey`. Use local host label `localhost` for M7 instead of inferring machine identity from evidence.
7. Seed `health_check_definitions` only from typed `Discovery.health_hints`; never parse `Evidence.summary`.
8. Create no `health_runs`, no `health_results`, and no dependencies during legacy migration.
9. Insert the `m7_health_dependency_systems_v1` marker only after every row has converted successfully.
10. Commit once. Any error before commit rolls back every M7 schema/data change.

For a fresh database with no imported rows, the same migration creates the empty M7 schema and marker in one transaction. Reopening a migrated database sees the marker and performs no duplicate conversion.

- [ ] **Step 1: Add failing legacy migration tests**

In `storage.rs`, add a helper that creates the exact pre-M7 schema and inserts reviewed discoveries plus legacy `inventory` rows before calling `Store::from_connection(connection)`. Do not use the new Store initializer to create the fixture because that would accidentally test the already-migrated schema.

Add tests named with the `m7_migration_` prefix that prove:

```rust
#[test]
fn m7_migration_preserves_reviewed_discovery_linkage_and_starts_grey() {
    let connection = legacy_connection_with_imported_discovery(
        legacy_discovery("Legacy CLI", "scanner-a", "fingerprint-a"),
    );
    let store = Store::from_connection(connection).unwrap();

    let capabilities = store.capabilities().unwrap();
    let installations = store.installations().unwrap();

    assert_eq!(capabilities.len(), 1);
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].aggregate_health, HealthState::Grey);
    assert_eq!(installations[0].source_discovery_id, legacy_discovery_id());
    assert!(store.health_results_for_installation(installations[0].id, 20).unwrap().is_empty());
    assert!(store.dependencies().unwrap().is_empty());
}
```

Add `m7_migration_groups_only_matching_trusted_canonical_identity`: create two imported rows with different discovery fingerprints but the same explicit `canonical_identity`; assert one capability and two installations.

Add `m7_migration_does_not_group_ambiguous_legacy_rows`: create two imported rows with identical suggested names and similar fingerprints but `canonical_identity = None`; assert two capabilities and two installations.

Add `m7_migration_seeds_definitions_only_from_typed_hints`: create one imported row containing a typed required `file_exists` hint and another legacy row whose evidence summary merely contains a path string; assert only the typed row receives a definition.

Add `m7_migration_is_idempotent`: open a legacy database through `Store::from_connection`, close/reopen the same database, and assert capability/installation/definition counts do not increase.

Add `m7_migration_failure_rolls_back_everything`: inject a malformed imported payload or another deterministic conversion failure after at least one valid legacy row; assert `Store::from_connection` returns an error and a raw inspection of that same connection/database shows no migration marker and no partially migrated capability or installation rows. If SQLite DDL rollback behavior makes table-existence assertions brittle, assert at minimum that the marker is absent and every M7 data table contains zero rows after reopening the raw connection.

- [ ] **Step 2: Run migration tests and verify RED**

Run: `cargo test -p control-center-core m7_migration_ --locked`

Expected: FAIL because the M7 schema, migration marker and canonical projections do not exist.

- [ ] **Step 3: Implement the transactional schema migration**

Refactor `Store::from_connection` so existing M6 table creation remains compatible, then invoke a private `migrate_m7_health_dependency_systems(&mut Connection) -> Result<(), StoreError>` before returning `Store`.

The migration function must check the marker first, perform all schema creation and legacy row conversion through one `rusqlite::Transaction`, insert the marker last, and commit exactly once. Do not call `Store` methods from inside the migration if those methods open nested transactions.

Create private typed encode/decode helpers for the repeated enum values rather than scattering string matches through query code. Use `serde_json` for `HealthTarget`.

- [ ] **Step 4: Implement canonical and installation read projections**

Implement `Store::capabilities()`, `Store::installations()` and `Store::health_check_definitions(installation_id)`. Order capabilities and installations deterministically by creation timestamp then id so frontend tests do not depend on SQLite incidental row order.

Keep `Store::inventory()` temporarily available only for existing M6 regression tests/internal compatibility until the frontend migration task removes its use. Do not use it as the new reviewed-inventory source.

- [ ] **Step 5: Run migration tests and verify GREEN**

Run: `cargo test -p control-center-core m7_migration_ --locked`

Expected: all migration tests PASS.

- [ ] **Step 6: Add failing tests for post-M7 review import**

Add `m7_review_import_creates_capability_installation_and_typed_definitions`: enqueue a pending Discovery carrying one deterministic canonical identity and one typed health hint, import it through `Store::review`, then assert one canonical capability, one installation linked to the reviewed discovery, Grey aggregate health and one matching definition.

Add `m7_review_import_reuses_only_exact_stable_identity`: import two reviewed discoveries carrying the same explicit `canonical_identity`; assert one capability and two installations. Then import another discovery with the same display name but no canonical identity and assert it creates a separate capability.

Add `m7_review_import_without_health_hints_stays_grey_without_definitions`: import a reviewed discovery containing only legacy evidence text and assert its installation exists, remains Grey and has zero definitions.

Add `m7_review_ignore_and_unknown_do_not_create_inventory_entities`: verify `Ignore` and `KeepUnknown` preserve the existing review-gate semantics and create neither capabilities nor installations.

- [ ] **Step 7: Run post-M7 import tests and verify RED**

Run: `cargo test -p control-center-core m7_review_ --locked`

Expected: FAIL because `Store::review` still inserts only the legacy inventory payload.

- [ ] **Step 8: Refactor Import review into one canonical-inventory transaction**

Inside `Store::review`, keep the existing pending-state validation and review-state update. For `ReviewDecision::Import`, in the same SQLite transaction:
1. deserialize the reviewed Discovery;
2. preserve the legacy `inventory` insert for migration provenance/backward compatibility;
3. resolve or create a canonical capability using only non-empty `canonical_identity`;
4. create exactly one installation for this `source_discovery_id` with `aggregate_health = Grey`;
5. seed deduplicated health definitions only from typed `health_hints`;
6. do not create health history;
7. defer scanner-owned dependency proposal materialization to the dependency task so relationship validation remains centralized.

The capability and installation writes must roll back together with the review-state mutation if any required insert fails. A failed import must never leave the discovery marked Imported while canonical inventory creation failed.

- [ ] **Step 9: Run post-M7 import tests and verify GREEN**

Run: `cargo test -p control-center-core m7_review_ --locked`

Expected: PASS.

- [ ] **Step 10: Run complete storage regression tests**

Run: `cargo test -p control-center-core storage --locked`

Expected: all existing M6 storage tests plus the new M7 migration/import tests PASS.

Pay particular attention to the existing invariants: resolved discoveries are not revived, repeated pending fingerprints keep ids aligned, ignored discoveries may return, scan ownership remains preserved and review is still mandatory before inventory import.

- [ ] **Step 11: Commit Task 2**

Run: `git add engine/rust/control-center-core/src/storage.rs`

Run: `git commit -m "feat: migrate reviewed inventory to canonical instances"`

Expected: one focused persistence commit containing the transactional M7 schema, legacy migration, canonical projections and post-M7 import behavior.

---

### Task 3: Deterministic health evaluation and generic local probes

**Files:**
- Create: `engine/rust/control-center-core/src/health.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `health.rs`

**Interfaces:**

- Consumes `HealthCheckDefinition`, `HealthCheckType`, `HealthTarget`, `HealthExpectedCondition`, `HealthState`, `HealthResult` and existing `redaction::redact`.
- Reuses the existing `ProcessSource`, `ServiceSource`, `WindowsProcessSource` and `WindowsServiceSource` abstractions from `windows.rs`.
- Produces `pub struct HealthProbeOutcome { pub state: HealthState, pub result_code: String, pub redacted_evidence: String }`.
- Produces `pub trait HealthProbeIo: Send + Sync { fn path_exists(&self, path: &Path) -> Result<bool, String>; fn file_exists(&self, path: &Path) -> Result<bool, String>; fn read_config(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, String>; fn process_running(&self, executable_path: &Path) -> Result<bool, String>; fn windows_service_running(&self, service_name: &str) -> Result<Option<bool>, String>; fn loopback_port_reachable(&self, host: &str, port: u16, timeout: Duration) -> Result<bool, String>; }`.
- Produces `pub struct NativeHealthProbeIo` implementing `HealthProbeIo` with read-only native operations.
- Produces `pub fn execute_health_check(definition: &HealthCheckDefinition, io: &dyn HealthProbeIo) -> HealthProbeOutcome`.
- Produces `pub fn is_result_stale(definition: &HealthCheckDefinition, result: &HealthResult, now: DateTime<Utc>) -> bool`.
- Produces `pub fn aggregate_health(definitions: &[HealthCheckDefinition], latest_results: &HashMap<Uuid, HealthResult>, now: DateTime<Utc>) -> HealthState`.
- Produces `pub const MAX_CONFIG_BYTES: usize = 1_048_576;` so `config_parses` never reads an unbounded file.

Probe-result codes are stable data codes, not transport errors. Use: `ok`, `path_missing`, `file_missing`, `setup_required`, `config_invalid_json`, `process_not_running`, `service_stopped`, `service_state_unknown`, `loopback_unreachable`, `health_check_unsupported`, `health_probe_error`.

Probe semantics:

| Check type | Valid target | Pass | Negative result |
| --- | --- | --- | --- |
| `path_exists` | `HealthTarget::Path` | Green / `ok` | Red / `path_missing` |
| `file_exists` | `HealthTarget::Path` | Green / `ok` | Red / `file_missing` |
| `config_present` | `HealthTarget::Config` | Green / `ok` | Blue / `setup_required` when missing |
| `config_parses` | `HealthTarget::Config { format: Json }` | Green / `ok` | Blue / `setup_required` when missing; Red / `config_invalid_json` when present but invalid |
| `process_running` | `HealthTarget::ProcessExecutable` | Green / `ok` | Red / `process_not_running` |
| `windows_service_state` | `HealthTarget::WindowsService` | Green / `ok` when running | Red / `service_stopped`; Grey / `service_state_unknown` when state cannot be determined |
| `loopback_port_reachable` | `HealthTarget::LoopbackSocket` | Green / `ok` | Red / `loopback_unreachable` |

A definition whose check type and target variant do not match returns Grey / `health_check_unsupported`. An operating-system or probe-I/O failure returns Grey / `health_probe_error`; it does not invent Red because the requested condition was not actually observed.

`loopback_port_reachable` must parse the host as `IpAddr`, reject every non-loopback address before opening a socket, and use only the definition timeout converted to a bounded `Duration`. No DNS resolution is used.

`config_present` checks metadata only. `config_parses` reads at most `MAX_CONFIG_BYTES`; a file larger than the bound returns Grey / `health_probe_error` with safe evidence rather than reading the rest. JSON parsing uses `serde_json::from_slice::<serde_json::Value>`.

`process_running` compares normalized exact executable paths obtained from `ProcessSource`; it does not fall back to process names. `windows_service_state` compares exact service names case-insensitively and never starts/stops a service.

Before returning any outcome, pass its evidence string through `redaction::redact`. Evidence should describe the check category and result without dumping config contents, authorization data, tokens, passwords or arbitrary file contents.

---

- [ ] **Step 1: Add failing aggregation and staleness tests**

Add tests named with the `health_aggregate_` prefix. Use small fixture builders for definitions/results so each test isolates one rule.

Required cases:

```rust
#[test]
fn health_aggregate_no_completed_required_checks_is_grey() {
    let definitions = vec![required_definition(HealthCheckType::FileExists)];
    assert_eq!(
        aggregate_health(&definitions, &HashMap::new(), now()),
        HealthState::Grey
    );
}

#[test]
fn health_aggregate_one_required_pass_cannot_hide_missing_required_result() {
    let definitions = vec![
        required_definition_with_id(id(1), HealthCheckType::FileExists),
        required_definition_with_id(id(2), HealthCheckType::ProcessRunning),
    ];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Green, "ok")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Grey);
}

#[test]
fn health_aggregate_required_failure_is_red() {
    let definitions = vec![required_definition_with_id(id(1), HealthCheckType::FileExists)];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Red, "file_missing")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Red);
}

#[test]
fn health_aggregate_required_setup_state_is_blue() {
    let definitions = vec![required_definition_with_id(id(1), HealthCheckType::ConfigPresent)];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Blue, "setup_required")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Blue);
}

#[test]
fn health_aggregate_required_unknown_is_grey() {
    let definitions = vec![required_definition_with_id(id(1), HealthCheckType::WindowsServiceState)];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Grey, "service_state_unknown")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Grey);
}

#[test]
fn health_aggregate_optional_failure_is_yellow_not_red() {
    let definitions = vec![
        required_definition_with_id(id(1), HealthCheckType::FileExists),
        optional_definition_with_id(id(2), HealthCheckType::ConfigParses),
    ];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Green, "ok")),
        (id(2), result_for(id(2), HealthState::Red, "config_invalid_json")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Yellow);
}

#[test]
fn health_aggregate_green_requires_every_required_check_to_pass() {
    let definitions = vec![
        required_definition_with_id(id(1), HealthCheckType::FileExists),
        required_definition_with_id(id(2), HealthCheckType::ProcessRunning),
    ];
    let latest = HashMap::from([
        (id(1), result_for(id(1), HealthState::Green, "ok")),
        (id(2), result_for(id(2), HealthState::Green, "ok")),
    ]);

    assert_eq!(aggregate_health(&definitions, &latest, now()), HealthState::Green);
}
```

Add `health_aggregate_precedence_is_red_blue_grey_yellow_green`: construct combinations proving Red wins over Blue, Blue over Grey, Grey over Yellow and Yellow over Green.

Add `health_aggregate_stale_required_pass_becomes_yellow`: give a required Green result older than its definition TTL and assert Yellow.

Add `health_aggregate_staleness_never_turns_red`: assert a stale Green result yields Yellow, not Red.

Add `health_aggregate_no_ttl_never_becomes_stale`: use a very old result with `stale_after_seconds = None` and assert it remains Green when every required result passes.

Add `health_result_stale_boundary_is_deterministic`: result age strictly greater than the configured TTL is stale; age equal to the TTL is not stale.

- [ ] **Step 2: Run aggregation tests and verify RED**

Run: `cargo test -p control-center-core health_aggregate_ --locked`

Expected: FAIL because `health.rs`, `aggregate_health` and `is_result_stale` do not exist.

- [ ] **Step 3: Implement deterministic staleness and aggregation**

Implement `is_result_stale` using only the definition TTL, persisted `observed_at` and supplied evaluation time. Do not call `Utc::now()` inside the pure aggregation function.

Implement aggregate evaluation in this order:
1. If there are no configured required definitions, return Grey.
2. For every required definition, obtain its latest applicable completed result by definition id. Missing required result contributes Grey.
3. A required Red contributes Red.
4. A required Blue contributes Blue.
5. A required Grey contributes Grey.
6. A required result in any unsupported/non-passing condition that cannot prove success contributes Grey unless an earlier stronger state already applies.
7. After all required checks are current Green, evaluate optional results and staleness. Any stale applicable result or optional non-Green result contributes Yellow.
8. Return Green only when every required definition has a non-stale Green result and no Yellow condition applies.

Apply precedence mechanically as Red > Blue > Grey > Yellow > Green rather than relying on enum declaration order.

- [ ] **Step 4: Run aggregation tests and verify GREEN**

Run: `cargo test -p control-center-core health_aggregate_ --locked`

Expected: all aggregation and staleness tests PASS.

**Blue-state semantics for M7 generic checks:** A missing required `config_present` target is explicit setup/user action required and returns `HealthState::Blue` with result code `setup_required`. For `config_parses`, a missing config also returns Blue / `setup_required`, while a present but invalid JSON file returns Red / `config_invalid_json`. This gives the approved Blue state a concrete generic M7 meaning without adding product-specific adapter logic.

- [ ] **Step 5: Add failing generic probe tests**

Add tests named with the `health_probe_` prefix using a fake `HealthProbeIo`. Required cases:

- `health_probe_path_exists_passes_when_directory_exists`: Green / `ok`.
- `health_probe_path_exists_fails_when_missing`: Red / `path_missing`.
- `health_probe_file_exists_passes_only_for_file`: Green for an existing file, Red / `file_missing` otherwise.
- `health_probe_config_present_missing_is_blue_setup_required`: Blue / `setup_required`.
- `health_probe_config_parses_valid_json_is_green`: Green / `ok`.
- `health_probe_config_parses_missing_is_blue_setup_required`: Blue / `setup_required`.
- `health_probe_config_parses_invalid_json_is_red`: Red / `config_invalid_json`.
- `health_probe_config_read_is_bounded`: a file exceeding `MAX_CONFIG_BYTES` returns Grey / `health_probe_error` without reading unbounded content.
- `health_probe_process_running_uses_exact_executable_path`: exact normalized executable path passes; name-only similarity does not.
- `health_probe_process_not_running_is_red`: Red / `process_not_running`.
- `health_probe_service_running_is_green`: Green / `ok`.
- `health_probe_service_stopped_is_red`: Red / `service_stopped`.
- `health_probe_service_unknown_is_grey`: Grey / `service_state_unknown`.
- `health_probe_loopback_reachable_is_green`: Green / `ok`.
- `health_probe_loopback_unreachable_is_red`: Red / `loopback_unreachable`.
- `health_probe_rejects_non_loopback_before_socket_io`: non-loopback target returns Grey / `health_check_unsupported` and the fake socket method is never called.
- `health_probe_mismatched_target_is_grey_unsupported`: mismatched check/target variant never executes I/O.
- `health_probe_io_error_is_grey_not_red`: operational inability to observe returns Grey / `health_probe_error`.
- `health_probe_redacts_evidence_before_return`: fixture evidence containing `token=secret-value`, `password=hunter2` and an Authorization bearer value contains `[REDACTED]` and none of the secret values.

- [ ] **Step 6: Run probe tests and verify RED**

Run: `cargo test -p control-center-core health_probe_ --locked`

Expected: FAIL because generic probe execution does not exist.

- [ ] **Step 7: Implement `HealthProbeIo`, `NativeHealthProbeIo` and `execute_health_check`**

Keep validation before I/O: reject a mismatched target variant and non-loopback socket target without invoking the underlying operation. Convert all probe errors into safe Grey health outcomes rather than propagating raw local errors into persisted evidence.

For native filesystem checks use `std::fs::metadata`/`is_file` without mutating the path. For JSON parsing, check metadata size first, refuse files larger than `MAX_CONFIG_BYTES`, read only the bounded file and parse to `serde_json::Value` without preserving contents.

For native process checks, enumerate with the existing Windows process abstraction and match only normalized executable paths. For services, enumerate with the existing service abstraction and compare exact service names case-insensitively. Do not duplicate ToolHelp or Service Control Manager FFI.

For loopback sockets, require an `IpAddr` whose `is_loopback()` is true, build a `SocketAddr` directly and use a bounded connect timeout. Never resolve hostnames and never connect to wildcard, LAN or public addresses.

Every outcome evidence string must be concise, category-specific and run through `redaction::redact` before returning. Do not persist config contents or arbitrary OS error strings.

- [ ] **Step 8: Run probe tests and verify GREEN**

Run: `cargo test -p control-center-core health_probe_ --locked`

Expected: all generic probe tests PASS.

- [ ] **Step 9: Export tested health contracts**

Modify `lib.rs`:

```rust
pub mod health;
pub use health::{
    HealthProbeIo, HealthProbeOutcome, MAX_CONFIG_BYTES, NativeHealthProbeIo,
    aggregate_health, execute_health_check, is_result_stale,
};
```

Do not add health-run orchestration exports yet.

- [ ] **Step 10: Run the complete health module test set**

Run: `cargo test -p control-center-core health_ --locked`

Expected: aggregation, staleness, probe, loopback-safety and redaction tests all PASS.

- [ ] **Step 11: Run core regression tests**

Run: `cargo test -p control-center-core --locked`

Expected: PASS with no M6 regressions.

- [ ] **Step 12: Commit Task 3**

Run: `git add engine/rust/control-center-core/src/health.rs engine/rust/control-center-core/src/lib.rs`

Run: `git commit -m "feat: add generic local health evaluation"`

Expected: one focused commit containing pure health semantics and the seven read-only generic probes.

---

### Task 4: Durable health history and authoritative health projections

**Files:**
- Modify: `engine/rust/control-center-core/src/model.rs`
- Modify: `engine/rust/control-center-core/src/storage.rs`
- Test: inline `#[cfg(test)]` module in `storage.rs`

**Interfaces:**

Add projection types in `model.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheckSnapshot {
    pub definition: HealthCheckDefinition,
    pub latest_result: Option<HealthResult>,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallationHealthSnapshot {
    pub installation: InstallationInstance,
    pub capability: CanonicalCapability,
    pub checks: Vec<HealthCheckSnapshot>,
    pub effective_health: HealthState,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub warning_count: u64,
    pub failure_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthSummary {
    pub capability_count: u64,
    pub installation_count: u64,
    pub pending_discovery_count: u64,
    pub green_count: u64,
    pub yellow_count: u64,
    pub red_count: u64,
    pub grey_count: u64,
    pub blue_count: u64,
}
```

Add Store methods:

```rust
pub fn begin_health_run(&mut self, run: &HealthRun) -> Result<(), StoreError>;
pub fn update_health_run(&mut self, run: &HealthRun) -> Result<(), StoreError>;
pub fn health_run(&self, run_id: Uuid) -> Result<HealthRun, StoreError>;
pub fn record_health_result_and_recompute(
    &mut self,
    result: &HealthResult,
    evaluated_at: DateTime<Utc>,
) -> Result<HealthState, StoreError>;
pub fn installation_health(
    &self,
    installation_id: Uuid,
    evaluated_at: DateTime<Utc>,
) -> Result<InstallationHealthSnapshot, StoreError>;
pub fn health_summary(
    &self,
    evaluated_at: DateTime<Utc>,
) -> Result<HealthSummary, StoreError>;
pub fn health_results_for_installation(
    &self,
    installation_id: Uuid,
    limit: usize,
) -> Result<Vec<HealthResult>, StoreError>;
```

`record_health_result_and_recompute` is one SQLite transaction: insert the append-only result, load the latest applicable result for every configured definition, call the pure `aggregate_health`, persist the resulting installation aggregate and commit. The caller may emit an event only after this method returns successfully.

A cancelled in-flight check does not create a `HealthResult`; cancellation is represented by the `HealthRun` lifecycle. This preserves the prior applicable result and prevents cancellation from replacing current health. A timeout that actually completes as an observed check outcome may create a Grey result with `health_check_timeout` because it proves the new observation was insufficient, not cancelled.

Read projections re-evaluate TTL staleness using the supplied `evaluated_at`. They must not mutate SQLite merely because time passed. `InstallationHealthSnapshot.effective_health` can therefore be Yellow because of staleness even when the last persisted `installation_instances.aggregate_health` was Green.

`warning_count` counts current optional non-Green outcomes plus stale applicable outcomes without double-counting one check. `failure_count` counts current required Red outcomes only. Grey and Blue are shown through `effective_health` but are not mislabeled as failures.

`last_checked_at` is the latest applicable persisted result timestamp for that installation, or `None` when no check has completed.

`health_results_for_installation` returns newest first and clamps `limit` to a small backend maximum of 100 so the compact-history API cannot request an unbounded result set.

---

- [ ] **Step 1: Add failing durable-health persistence tests**

Add tests named with the `health_storage_` prefix.

Add `health_storage_results_are_append_only`: begin one run, record two completed results for the same definition at different timestamps, then assert both history rows remain present and newest is returned first.

Add `health_storage_result_and_aggregate_commit_together`: create two required definitions, persist one Green result and assert persisted aggregate remains Grey because the second required result is missing; persist the second Green result and assert aggregate becomes Green.

Add `health_storage_required_failure_persists_red`: persist a required Red result and assert the installation aggregate persisted in `installation_instances` becomes Red.

Add `health_storage_optional_failure_persists_yellow`: with all required checks Green, persist an optional Red result and assert aggregate becomes Yellow rather than Red.

Add `health_storage_cancelled_run_preserves_prior_result`: create a prior completed Green result, start a later run and mark that run Cancelled without inserting another result; assert the prior Green result remains the latest applicable result.

Add `health_storage_timeout_can_persist_grey`: persist a Grey result with `result_code = "health_check_timeout"` and assert it remains append-only and contributes Grey for a required definition.

Add `health_storage_projection_recomputes_staleness_without_writing`: persist a Green result for a definition with TTL, read immediately and assert Green, then read with a later supplied evaluation time and assert effective Yellow while the stored installation aggregate remains unchanged.

Add `health_storage_summary_counts_installations_not_capabilities`: create one capability with two installations in different health states plus one pending discovery; assert `capability_count == 1`, `installation_count == 2`, health-state counts total 2 and `pending_discovery_count == 1`.

Add `health_storage_last_checked_and_counts_are_current_projection`: assert `last_checked_at`, `warning_count` and `failure_count` derive from the latest applicable results rather than all historical rows.

Add `health_storage_history_limit_is_bounded`: insert more than 100 historical results, request a larger limit and assert at most 100 newest rows are returned.

- [ ] **Step 2: Run storage tests and verify RED**

Run: `cargo test -p control-center-core health_storage_ --locked`

Expected: FAIL because durable health-run/result methods and projection types are not implemented.

- [ ] **Step 3: Implement health-run persistence**

Implement `begin_health_run`, `update_health_run` and `health_run`. Validate lifecycle values through typed enums and keep run revisions opaque strings. `update_health_run` updates only the run row supplied by the caller and must not synthesize result rows.

- [ ] **Step 4: Implement append-only result persistence plus aggregate transaction**

Implement `record_health_result_and_recompute` as one transaction. Insert the result first, query the newest result per configured definition, call `aggregate_health`, update `installation_instances.aggregate_health` plus `updated_at`, then commit. Never update or delete a prior `health_results` row.

If any insert, decode, aggregate-input query or aggregate update fails, roll back the entire result/aggregate transaction so an emitted event can never describe a result that SQLite did not commit.

- [ ] **Step 5: Implement authoritative health projections**

Implement `installation_health`, `health_summary` and `health_results_for_installation`. Re-evaluate staleness from the caller-supplied evaluation time, calculate effective state in Rust, and do not mutate the database merely because TTL elapsed.

For latest-result selection use `observed_at DESC` with deterministic id tie-breaking. Do not use an arbitrary historical Red result after a newer completed result exists for the same definition.

Summary health counts are by installation instance. Capability count and pending-discovery count are separate fields and must never be mixed into health-state totals.

- [ ] **Step 6: Run durable-health tests and verify GREEN**

Run: `cargo test -p control-center-core health_storage_ --locked`

Expected: all append-only, aggregate, staleness, summary and bounded-history tests PASS.

- [ ] **Step 7: Run health plus storage regression suites**

Run: `cargo test -p control-center-core health_ --locked`

Expected: PASS.

Run: `cargo test -p control-center-core storage --locked`

Expected: PASS.

- [ ] **Step 8: Commit Task 4**

Run: `git add engine/rust/control-center-core/src/model.rs engine/rust/control-center-core/src/storage.rs`

Run: `git commit -m "feat: persist health history and projections"`

Expected: one focused commit containing durable health runs/results, atomic aggregate persistence and authoritative read projections.

---

### Task 5: Bounded cancellable health-run orchestration

**Files:**
- Create: `engine/rust/control-center-core/src/health_control.rs`
- Modify: `engine/rust/control-center-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `health_control.rs`

**Interfaces:**

- Consumes `HealthCheckDefinition`, `HealthProbeOutcome`, `HealthResult`, `HealthRunScope`, `HealthRunState`, `HealthState`, `execute_health_check`, Tokio tasks/timeouts and `tokio_util::sync::CancellationToken`.
- Produces `pub const HEALTH_INSTALLATION_CONCURRENCY: usize = 4`.
- Produces `pub trait HealthCheckExecutor: Send + Sync { fn execute(&self, definition: &HealthCheckDefinition) -> HealthProbeOutcome; }`.
- Produces `pub struct NativeHealthCheckExecutor` wrapping `NativeHealthProbeIo`.
- Produces `pub enum HealthEvent { Started { run_id: Uuid, scope: HealthRunScope, requested_installation_count: u64 }, CheckCompleted { run_id: Uuid, result: HealthResult }, Cancelled { run_id: Uuid, completed_installation_count: u64 }, Completed { run_id: Uuid, completed_installation_count: u64 } }`.
- Produces `pub struct HealthEventSink` backed by a bounded Tokio channel. Health events are lossless for M7; do not silently drop result or lifecycle events.
- Produces `pub struct InstallationHealthWork { pub installation_id: Uuid, pub definitions: Vec<HealthCheckDefinition> }`.
- Produces `pub async fn run_health_work(run_id: Uuid, scope: HealthRunScope, work: Vec<InstallationHealthWork>, executor: Arc<dyn HealthCheckExecutor>, cancellation: CancellationToken, events: HealthEventSink)`.

`health_control.rs` owns execution order, timeouts, cancellation checkpoints and installation concurrency only. It does not open SQLite and does not hold a `Store`. Persistence remains the desktop event bridge in Task 7.

Each installation worker executes its definitions sequentially in deterministic order by definition id. Check All runs at most `HEALTH_INSTALLATION_CONCURRENCY = 4` installation workers concurrently. Different installations may progress independently.

Every individual probe runs through `tokio::task::spawn_blocking` because filesystem/process/service/config/socket operations are synchronous. Wrap awaiting that task in `tokio::time::timeout(Duration::from_millis(definition.timeout_ms), ...)`. A timeout produces one Grey `HealthResult` with `result_code = "health_check_timeout"` and safe evidence.

Cancellation is checked before admitting an installation worker and before starting every definition. If cancellation has already won, no new probe starts. A probe already executing may finish because blocking native work is not force-killed; after cancellation wins, discard a late outcome rather than publishing a new completed result for work that had not settled before cancellation. All native operations themselves remain bounded by the definition timeout or their local bounded-read/enumeration constraints.

Cancellation emits one terminal `Cancelled` event after already-settled result events have been sent. Normal completion emits one terminal `Completed` event. An individual Red/Grey/Blue health result never fails the overall run.

Do not add pause/resume to health runs. M7 health supports Running -> Completed or Running -> Cancelling -> Cancelled only.

---

**Core/frontend event boundary:** `health_control.rs` does not emit `InstallationUpdated` because it has no authoritative persisted aggregate. Task 7 converts a persisted `CheckCompleted` into frontend `health.check_completed`, then emits `health.installation_updated` only after `Store::record_health_result_and_recompute` returns the committed aggregate.

- [ ] **Step 1: Add failing coordinator tests**

Add tests named with the `health_control_` prefix using deterministic fake executors, atomics and synchronization barriers rather than arbitrary sleeps as the primary proof.

Add `health_control_check_timeout_does_not_block_later_checks`: configure the first definition to block beyond its timeout and a second definition to finish immediately; assert one Grey `health_check_timeout` result is emitted for the first and the second still completes.

Add `health_control_slow_installation_does_not_block_unrelated_installation`: block one installation executor on a barrier while another installation is allowed to finish; assert the unrelated installation emits its completed result before the blocked worker is released.

Add `health_control_check_all_never_exceeds_four_installations`: run at least eight installation work items using a fake executor that tracks current and maximum simultaneous installation activity; assert the observed maximum is `<= HEALTH_INSTALLATION_CONCURRENCY` and reaches 4 when enough work is available.

Add `health_control_cancellation_preserves_already_completed_results`: allow the first check to complete and be emitted, block the next check, cancel the token, then assert the already-emitted result remains in the event stream and terminal state is Cancelled.

Add `health_control_cancellation_prevents_new_checks`: cancel immediately after a controlled first check settles and assert later definitions for that installation are never passed to the executor.

Add `health_control_cancellation_prevents_new_installation_admission`: hold four workers active, cancel before releasing them and assert queued installations never start.

Add `health_control_late_blocking_outcome_after_cancel_is_discarded`: start one blocking probe, cancel while it is in flight, release it afterward and assert no `CheckCompleted` event is emitted for that late outcome.

Add `health_control_result_states_do_not_fail_batch`: return Red, Blue and Grey outcomes from separate checks and assert the overall run still emits `Completed`, not a failed lifecycle.

Add `health_control_terminal_event_is_last`: collect all events for a completed and a cancelled run and assert no result event appears after the terminal event.

Add `health_control_definition_order_is_deterministic_within_installation`: provide definitions in reverse UUID order and assert executor invocation order is ascending definition id.

- [ ] **Step 2: Run coordinator tests and verify RED**

Run: `cargo test -p control-center-core health_control_ --locked`

Expected: FAIL because `health_control.rs` and the coordinator contracts do not exist.

- [ ] **Step 3: Implement the bounded coordinator**

Create `health_control.rs` with `HEALTH_INSTALLATION_CONCURRENCY = 4`, `HealthCheckExecutor`, `NativeHealthCheckExecutor`, corrected core `HealthEvent`, `HealthEventSink`, `InstallationHealthWork` and `run_health_work`.

Use a Tokio semaphore or equivalent bounded scheduling primitive for installation-level concurrency. Do not spawn every installation and rely on the executor to serialize them. Check cancellation before acquiring/admitting new work and again immediately before each definition.

Run each synchronous probe in `spawn_blocking`. Race the join against both cancellation and the definition timeout with cancellation taking precedence when they become ready together. A timed-out blocking task may finish later, but its late value is detached and never emitted. A cancellation-losing late result is likewise discarded.

Generate `HealthResult.id`, `observed_at` and measured `duration_ms` only for outcomes that actually settle as completed or timeout results before cancellation wins.

- [ ] **Step 4: Run coordinator tests and verify GREEN**

Run: `cargo test -p control-center-core health_control_ --locked`

Expected: all timeout, isolation, concurrency and cancellation tests PASS.

- [ ] **Step 5: Export tested health orchestration contracts**

Modify `lib.rs`:

```rust
pub mod health_control;
pub use health_control::{
    HEALTH_INSTALLATION_CONCURRENCY, HealthCheckExecutor, HealthEvent, HealthEventSink,
    InstallationHealthWork, NativeHealthCheckExecutor, run_health_work,
};
```

Keep `scan_control` and `health_control` separate modules with separate event types and lifecycle state. Do not reuse scan pause gates, scan registries or scan revisions for health.

- [ ] **Step 6: Run the complete health-core suite**

Run: `cargo test -p control-center-core health_ --locked`

Expected: aggregation, probes, storage and orchestration tests PASS.

- [ ] **Step 7: Run the full core regression suite**

Run: `cargo test -p control-center-core --locked`

Expected: PASS, including all existing M6 scan, Windows discovery, Python supervisor and storage tests.

- [ ] **Step 8: Commit Task 5**

Run: `git add engine/rust/control-center-core/src/health_control.rs engine/rust/control-center-core/src/lib.rs`

Run: `git commit -m "feat: add cancellable health orchestration"`

Expected: one focused commit containing bounded installation concurrency, per-check timeouts and cancellation semantics only.

---

### Task 6: Review-gated dependency persistence and ownership rules

**Files:**
- Modify: `engine/rust/control-center-core/src/storage.rs`
- Test: inline `#[cfg(test)]` module in `storage.rs`

**Interfaces:**

- Consumes `DependencyKind`, `DependencyOwner`, `DependencyProposal`, `DependencyRelationship`, canonical capabilities and reviewed Discovery metadata.
- Produces `Store::dependencies() -> Result<Vec<DependencyRelationship>, StoreError>`.
- Produces `Store::create_user_dependency(source_capability_id: Uuid, kind: DependencyKind, target_capability_id: Uuid) -> Result<DependencyRelationship, StoreError>`.
- Produces `Store::delete_user_dependency(relationship_id: Uuid) -> Result<(), StoreError>`.
- Adds private transaction-scoped scanner proposal materialization used only by `ReviewDecision::Import`.

Extend `StoreError` with typed dependency cases rather than inferring them from SQLite strings: `DependencyInvalid`, `DependencyDuplicate`, `DependencySelfReference`, `DependencyNotUserOwned`. Preserve existing variants.

Dependency invariants:
- Source and target capabilities must both exist.
- Source and target must differ.
- Kind must already be a typed `DependencyKind`; no arbitrary strings enter the Store API.
- A logical duplicate is the same `(source_capability_id, target_capability_id, kind)` regardless of owner.
- User creation sets owner `User`, provenance `user`, empty evidence summary, current timestamp and a fresh opaque relationship revision.
- User deletion succeeds only when owner is `User`.
- Scanner-owned relationships cannot be deleted through the user mutation method.
- Relationship list order is deterministic by creation timestamp then id.
- Scanner evidence summaries pass through `redaction::redact` before persistence.
- User-owned relationships are never rewritten, re-owned or silently removed by scanner proposal processing.

Scanner proposal materialization occurs inside the same `Store::review(..., Import)` transaction after the source canonical capability has been resolved or created. For each typed proposal:
1. accept only `Requires` or `DependsOn`;
2. source capability is the capability created/resolved for the imported discovery;
3. trim `target_stable_identity` and reject empty identity;
4. resolve target only by exact `canonical_capabilities.stable_identity`;
5. if zero targets resolve, create nothing;
6. database uniqueness guarantees more than one exact target cannot resolve; if persisted state is nevertheless inconsistent, return storage integrity rather than guessing;
7. reject self-reference;
8. if the same logical relationship already exists, leave it unchanged regardless of owner;
9. otherwise insert owner `Scanner`, provenance `scanner:<source_scanner>:<discovery_id>`, redacted proposal evidence and a fresh revision.

An unresolved scanner proposal is not queued for heuristic resolution and does not become active later merely because a similarly named capability appears. A future reviewed discovery may propose it again.

---

- [ ] **Step 1: Add failing dependency persistence tests**

Add tests named with the `dependency_storage_` prefix.

Add `dependency_storage_user_requires_persists`: create two canonical capabilities, call `create_user_dependency(source, Requires, target)`, then assert kind `Requires`, owner `User`, provenance `user` and list persistence.

Add `dependency_storage_user_depends_on_persists`: same for `DependsOn`.

Add `dependency_storage_rejects_self_reference`: use the same capability for source and target and assert `StoreError::DependencySelfReference`.

Add `dependency_storage_rejects_logical_duplicate`: create one relationship, then attempt the same source, target and kind again and assert `StoreError::DependencyDuplicate`.

Add `dependency_storage_missing_source_or_target_is_invalid`: use unknown UUIDs and assert `StoreError::DependencyInvalid` without creating any row.

Add `dependency_storage_user_can_delete_user_owned`: create a user relationship, delete it and assert it no longer appears.

Add `dependency_storage_user_cannot_delete_scanner_owned`: insert/materialize a scanner relationship, call `delete_user_dependency`, assert `StoreError::DependencyNotUserOwned` and verify the row remains.

Add `dependency_storage_scanner_proposal_materializes_only_on_import`: enqueue a discovery with a typed proposal to an already-existing exact target stable identity; assert no relationship before review, then import and assert one owner-Scanner relationship appears.

Add `dependency_storage_scanner_proposal_unresolved_target_creates_nothing`: import a proposing discovery whose target identity does not resolve exactly and assert zero relationships.

Add `dependency_storage_scanner_proposal_rejects_self_reference`: proposal points to the importing discovery capability identity and assert no scanner relationship is created.

Add `dependency_storage_scanner_proposal_does_not_replace_existing_user_relationship`: pre-create the same logical relationship as owner User, then import a scanner proposal for it; assert exactly one relationship remains and owner is still User.

Add `dependency_storage_scanner_activity_preserves_unrelated_user_relationships`: create multiple user relationships, import another discovery with scanner proposals and assert all pre-existing user rows remain byte-for-byte unchanged in owner, kind, endpoints and revision.

Add `dependency_storage_scanner_evidence_is_redacted`: proposal evidence contains token/password/Bearer fixture data; after import assert persisted evidence contains `[REDACTED]` and no secret values.

- [ ] **Step 2: Run dependency tests and verify RED**

Run: `cargo test -p control-center-core dependency_storage_ --locked`

Expected: FAIL because dependency Store APIs and review-time proposal materialization are not implemented.

- [ ] **Step 3: Implement user dependency CRUD**

Implement `dependencies`, `create_user_dependency` and `delete_user_dependency`. Resolve source and target existence explicitly before insert. Detect self-reference before hitting SQLite. Map uniqueness violations for the logical relationship key to `StoreError::DependencyDuplicate`, not a generic database string.

Deletion first loads the relationship by id, returns `NotFound` when absent, rejects owner Scanner with `DependencyNotUserOwned`, and deletes only owner User.

- [ ] **Step 4: Implement scanner proposal materialization inside import review**

Add a private helper that receives the active review transaction, imported Discovery and resolved source capability id. Resolve proposal targets only by exact non-empty stable identity and insert owner Scanner relationships only when all invariants pass.

Unresolved targets, self-reference and existing logical duplicates are safe skips for scanner proposal materialization rather than import failures. Persisted-state corruption or serialization failures remain real Store errors and roll back the import transaction.

Never update an existing relationship row to change its owner. Never delete a user-owned row because scanner evidence changed.

- [ ] **Step 5: Run dependency tests and verify GREEN**

Run: `cargo test -p control-center-core dependency_storage_ --locked`

Expected: all dependency persistence, ownership, review-gate and redaction tests PASS.

- [ ] **Step 6: Run storage regression tests**

Run: `cargo test -p control-center-core storage --locked`

Expected: PASS, including migration, review, health storage and dependency tests.

- [ ] **Step 7: Commit Task 6**

Run: `git add engine/rust/control-center-core/src/storage.rs`

Run: `git commit -m "feat: add reviewed dependency relationships"`

Expected: one focused commit containing dependency persistence, ownership rules and scanner proposal materialization through the review gate.

---

### Task 7: Tauri health registry, commands and persistence bridge

**Files:**
- Create: `apps/desktop/src-tauri/src/health_commands.rs`
- Modify: `apps/desktop/src-tauri/src/scan_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `health_commands.rs`

**Interfaces:**

- Consumes core `HealthEvent`, `HealthRun`, `HealthRunScope`, `HealthRunState`, `InstallationHealthWork`, `NativeHealthCheckExecutor`, `run_health_work`, Store health projections and `CancellationToken`.
- Keeps scan and health registries independent.
- Produces one frontend event channel named `health:event`.
- Produces stable command error codes `health_run_active`, `health_check_timeout`, `health_check_cancelled`, `health_check_unsupported`, plus existing `conflict`, `storage_integrity`, `invalid_request` where appropriate. Negative health outcomes remain result data and are not returned as command errors.
- Produces `HealthRunHandle { run_id, scope, state, revision, started_at, requested_installation_count, scheduled_installation_count, skipped_active_installation_count }`.
- Produces `HealthMutation { run_id, revision }` and `HealthRunStateResponse { run_id, state, revision }`.
- Produces `HealthRegistry` with an opaque workspace revision, per-run revisions, cancellation tokens, active run metadata and installation ownership.

Health registry rules:
- One active health run may own an installation at a time.
- `run_health_checks` for an installation already owned by another active run returns `health_run_active` without mutation.
- `run_all_health_checks` skips installations already owned by another run rather than duplicating them.
- `requested_installation_count` records the number of reviewed installations considered by the user request.
- `scheduled_installation_count` records the installations actually admitted to this run.
- `skipped_active_installation_count` reports already-owned installations skipped by Check All.
- A Check All request with zero reviewed installations returns a valid immediately-completing run with zero counts rather than fabricating work.
- A Check All request where every reviewed installation is already active may complete immediately with every installation reported as skipped.
- Accepted run start rotates the health workspace revision.
- Each accepted cancel state change rotates that run revision.
- Repeating cancel with the current revision after Cancelling is a safe no-op returning current state without another rotation.
- Stale workspace or run revision returns `conflict` without mutation.
- Health revisions are completely independent of `scanRevision` and scan per-run revisions.

The registry must support synchronous admission without holding the Store lock during probes. Admission loads reviewed installation ids and their definitions through short-lived Store access, releases Store, then launches the core coordinator with owned `InstallationHealthWork`.

Move shared application-state ownership only as much as needed so `AppState` can contain `store`, existing `scans` and new `health`. Do not merge the two registries into one lifecycle abstraction.

---

Add Tauri-facing projection and request types in `health_commands.rs`:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthWorkspaceSnapshot {
    pub summary: HealthSummary,
    pub installations: Vec<InstallationHealthSnapshot>,
    pub revision: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationHealthDetail {
    pub snapshot: InstallationHealthSnapshot,
    pub history: Vec<HealthResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInstallationHealthRequest {
    pub installation_id: String,
    pub revision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAllHealthRequest {
    pub revision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelHealthRequest {
    pub run_id: String,
    pub revision: String,
}
```

Expose these commands:

```rust
#[tauri::command]
pub fn list_health_summary(
    state: State<'_, AppState>,
) -> Result<HealthWorkspaceSnapshot, HealthCommandError>;

#[tauri::command]
pub fn list_installation_health(
    installation_id: String,
    state: State<'_, AppState>,
) -> Result<InstallationHealthDetail, HealthCommandError>;

#[tauri::command]
pub async fn run_health_checks(
    request: RunInstallationHealthRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<HealthRunHandle, HealthCommandError>;

#[tauri::command]
pub async fn run_all_health_checks(
    request: RunAllHealthRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<HealthRunHandle, HealthCommandError>;

#[tauri::command]
pub fn cancel_health_run(
    request: CancelHealthRequest,
    state: State<'_, AppState>,
) -> Result<HealthRunStateResponse, HealthCommandError>;
```

Use a dedicated `HealthCommandError { code: &'static str, message: String, state: Option<HealthRunStateResponse> }`. Do not change the existing scan `CommandError` wire shape merely to reuse it. Map Store/SQLite internals to fixed safe text such as `Local storage is unavailable`; never use `StoreError::to_string()` as an IPC message.

Frontend `health:event` payloads use tagged camelCase structs with wire `kind` values exactly `started`, `check_completed`, `installation_updated`, `cancelled`, `completed`. Every payload contains `runId`. `check_completed` includes the persisted `HealthResult`. `installation_updated` includes `installationId` plus the committed aggregate state.

Add a Tauri-free persistence bridge abstraction mirroring the proven M6 test pattern:

```rust
trait HealthEventBridgePorts {
    fn persist_started(&self, run: &HealthRun) -> Result<(), String>;
    fn persist_result(&self, result: &HealthResult) -> Result<HealthState, String>;
    fn persist_terminal(&self, run: &HealthRun) -> Result<(), String>;
    fn emit(&self, event: FrontendHealthEvent) -> Result<(), String>;
    fn finish_registry_run(&self, run_id: Uuid);
}
```

Bridge ordering is exact:
- Started: persist `HealthRun { state: Running }`, then emit `started`.
- CheckCompleted: call `Store::record_health_result_and_recompute`, then emit `check_completed`, then emit `installation_updated` with the returned committed aggregate.
- Cancelled: persist terminal run state Cancelled, then emit `cancelled`, then release registry ownership exactly once.
- Completed: persist terminal run state Completed, then emit `completed`, then release registry ownership exactly once.

No frontend health event describing durable state may be emitted before its corresponding SQLite operation succeeds.

**Lock order:** health commands may read/clone installation work from Store first, but must release the Store mutex before acquiring HealthRegistry locks. Registry admission may invoke a short persistence closure that locks Store only after no Store guard from the caller remains. `bootstrap_state` and health read commands read registry revisions before taking Store. Event persistence and registry cleanup occur sequentially, never while holding both locks. Add a regression test for this order.

---

- [ ] **Step 1: Add failing health registry and command tests**

Add tests named with the `health_command_` prefix. Keep registry tests Tauri-free where possible and use fake bridge ports for ordering assertions.

Add `health_command_single_run_claims_only_requested_installation`: admit one installation and assert its ownership is recorded while unrelated installations remain available.

Add `health_command_duplicate_single_run_returns_active_error`: start a run owning one installation, attempt another single run for the same installation and assert `health_run_active` without changing either revision or ownership.

Add `health_command_check_all_skips_installations_owned_by_other_runs`: pre-own one installation, request Check All over three reviewed installations and assert requested count 3, scheduled count 2 and skipped-active count 1.

Add `health_command_check_all_respects_zero_inventory`: with no reviewed installations, assert an accepted immediately-completing run with all counts zero and no fabricated results.

Add `health_command_check_all_all_active_completes_without_duplicate_work`: pre-own every reviewed installation, request Check All and assert scheduled count 0, skipped count equals requested count and no second executor invocation occurs.

Add `health_command_stale_workspace_revision_is_conflict_without_mutation`: submit a stale revision and assert code `conflict`, unchanged workspace revision and no ownership change.

Add `health_command_start_rotates_workspace_revision`: accepted single and Check All starts each return a fresh workspace revision distinct from the prior value.

Add `health_command_cancel_rotates_run_revision_once`: cancel a Running run with its current run revision, assert state Cancelling and a new revision; repeat with the returned current revision and assert a no-op with the same revision.

Add `health_command_stale_cancel_revision_is_conflict`: cancel with an obsolete run revision and assert no token cancellation or lifecycle mutation.

Add `health_command_scan_and_health_revisions_are_independent`: mutate the scan registry and assert health workspace revision does not change; mutate health registry and assert scan revision does not change.

Add `health_command_persists_started_before_emitting_started`: fake bridge records call order and assert `persist_started` precedes `emit(started)`.

Add `health_command_persists_result_before_any_result_event`: fake bridge returns a committed aggregate; assert order is `persist_result`, `emit(check_completed)`, `emit(installation_updated)`.

Add `health_command_persist_result_failure_emits_nothing_for_that_result`: make `persist_result` fail and assert neither result event is emitted.

Add `health_command_terminal_persist_precedes_terminal_event_and_cleanup`: assert Completed and Cancelled each follow `persist_terminal`, then `emit`, then `finish_registry_run`.

Add `health_command_terminal_persist_failure_does_not_claim_durable_completion`: make terminal persistence fail and assert no terminal durable-state event is emitted; registry cleanup follows the explicit failure path rather than deadlocking or leaking ownership indefinitely.

Add `health_command_frontend_events_include_run_id`: serialize all five frontend health event variants and assert every payload includes `runId` and the documented camelCase `kind`.

Add `health_command_storage_errors_are_redacted_for_ipc`: inject Store failures containing a fake database path and secret token and assert the returned `HealthCommandError.message` exposes neither raw value.

Add `health_command_lock_order_does_not_deadlock_bootstrap_and_start`: reproduce the M6-style opposing-operation scenario with synchronization barriers, concurrently obtaining bootstrap/read state and starting health work; assert both finish within a bounded test timeout.

- [ ] **Step 2: Run Tauri health tests and verify RED**

Run: `cargo test -p control-center-desktop health_command_ --locked`

Expected: FAIL because `health_commands.rs`, HealthRegistry and Tauri health commands do not exist.

- [ ] **Step 3: Implement HealthRegistry admission and revisions**

Create the independent HealthRegistry with short critical sections only. Keep installation ownership in registry memory keyed by installation UUID and release ownership exactly once at terminal cleanup.

Validate workspace revision before any admission mutation. For a single installation, reject active ownership. For Check All, compute requested, scheduled and skipped sets deterministically before launch. Rotate workspace revision only after admission succeeds.

Create/persist the initial `HealthRun` before the frontend receives `started`. Do not hold a registry mutex while waiting for SQLite, probe execution or Tauri event emission.

- [ ] **Step 4: Implement command reads and launch paths**

Implement `list_health_summary` and `list_installation_health` using `Utc::now()` only at the command boundary, passing that timestamp into Store projection methods. Read the current health workspace revision without holding Store and return it with the snapshot.

For run commands:
1. parse UUIDs and validate revision;
2. load owned `InstallationHealthWork` from Store under a short Store lock;
3. drop Store guard;
4. admit work in HealthRegistry;
5. construct the persisted Running `HealthRun`;
6. launch the core coordinator and bridge task;
7. return `HealthRunHandle` without waiting for the run to finish.

Do not execute probes on the Tauri command future itself.

- [ ] **Step 5: Implement cancellation and bridge ordering**

`cancel_health_run` validates run id and run revision, transitions Running to Cancelling once, rotates run revision, triggers the run CancellationToken and returns immediately. The coordinator terminal event later persists Cancelled and releases ownership.

Implement the bridge ordering exactly as documented above. Map bridge persistence failures to safe logging/internal handling without emitting false durable-state events. Ensure cleanup cannot execute twice.

- [ ] **Step 6: Register module and commands**

In `lib.rs`, add `mod health_commands;`, initialize `HealthRegistry` in `AppState`, include health commands in `tauri::generate_handler!`, and cancel active health tokens during application/window shutdown alongside existing scan cancellation.

Update `bootstrap_state` only enough to expose the new canonical inventory and revision fields required by later frontend tasks. Do not add frontend rendering in this task.

Replace any newly touched `StoreError::to_string()` IPC paths with fixed safe error mapping. Do not broaden this refactor beyond touched bootstrap/review/health paths unless a test requires it.

- [ ] **Step 7: Run Tauri health tests and verify GREEN**

Run: `cargo test -p control-center-desktop health_command_ --locked`

Expected: all registry, revision, event-order, redaction and deadlock regression tests PASS.

- [ ] **Step 8: Run desktop regression tests**

Run: `cargo test -p control-center-desktop --locked`

Expected: PASS, including all existing M6 scan-command tests.

- [ ] **Step 9: Commit Task 7**

Run: `git add apps/desktop/src-tauri/src/health_commands.rs apps/desktop/src-tauri/src/scan_commands.rs apps/desktop/src-tauri/src/lib.rs`

Run: `git commit -m "feat: expose persisted health runs"`

Expected: one focused desktop commit containing health IPC, independent revisions, active-run ownership, persist-before-notify events and shutdown cancellation.

---

### Task 8: Tauri dependency commands and optimistic revisions

**Files:**
- Create: `apps/desktop/src-tauri/src/dependency_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `dependency_commands.rs`

**Interfaces:**

Add desktop projection/request types:

```rust
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyWorkspaceSnapshot {
    pub relationships: Vec<DependencyRelationship>,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserDependencyRequest {
    pub source_capability_id: String,
    pub kind: DependencyKind,
    pub target_capability_id: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUserDependencyRequest {
    pub relationship_id: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyMutationResponse {
    pub relationships: Vec<DependencyRelationship>,
    pub revision: String,
}
```

Expose commands:

```rust
#[tauri::command]
pub fn list_dependencies(
    state: State<'_, AppState>,
) -> Result<DependencyWorkspaceSnapshot, DependencyCommandError>;

#[tauri::command]
pub fn create_user_dependency(
    request: CreateUserDependencyRequest,
    state: State<'_, AppState>,
) -> Result<DependencyMutationResponse, DependencyCommandError>;

#[tauri::command]
pub fn delete_user_dependency(
    request: DeleteUserDependencyRequest,
    state: State<'_, AppState>,
) -> Result<DependencyMutationResponse, DependencyCommandError>;
```

Create an independent `DependencyRegistry` containing only an opaque workspace revision. It has no worker tasks, cancellation token or lifecycle state. It serializes optimistic mutations and rotates revision exactly once after each successful create/delete. Failed or stale mutations leave the revision unchanged.

`AppState` therefore owns three independent coordination domains: existing `scans`, new `health` and new `dependencies`. Do not share revisions between them.

Use `DependencyCommandError { code: &'static str, message: String }` with stable codes exactly `conflict`, `dependency_invalid`, `dependency_duplicate`, `dependency_self_reference`, `dependency_not_user_owned`, `not_found`, `storage_integrity`, `invalid_request`. Map Store errors explicitly. Never expose raw SQLite text, filesystem paths, evidence strings or secrets through command errors.

Mutation algorithm is exact:
1. parse UUID request fields before taking locks;
2. acquire DependencyRegistry briefly and verify the supplied revision;
3. release the registry lock before taking Store;
4. perform exactly one Store mutation;
5. release Store;
6. reacquire DependencyRegistry and rotate revision only if the revision still equals the validated revision;
7. if another dependency mutation won between steps 3 and 6, treat this as an internal serialization bug and do not silently overwrite its revision; implement registry mutation admission so only one dependency mutation can be in its admitted critical section at a time without holding the registry mutex across SQLite I/O;
8. return the authoritative relationship list plus new revision.

Implement this safely with a lightweight mutation permit/token stored in `DependencyRegistry`: successful revision validation marks one mutation id as in-flight, then releases the mutex. Completion with that same mutation id rotates revision and clears the permit. Failure clears the permit without rotating. A second mutation while a permit is active returns `conflict` and does not enter Store. This prevents lost updates without holding the registry lock during SQLite work.

`list_dependencies` reads the dependency revision and Store data without holding both locks simultaneously. The list is authoritative SQLite data and remains usable independently of the graph view.

Register all three dependency commands in `tauri::generate_handler!` and initialize `DependencyRegistry` with the application state. Include `dependencyRevision` in bootstrap state so the frontend can begin with a current mutation token.

---

- [ ] **Step 1: Add failing dependency command tests**

Add tests named with the `dependency_command_` prefix. Keep revision/permit tests Tauri-free where possible.

Add `dependency_command_list_returns_authoritative_relationships_and_revision`: seed relationships in Store and assert list output contains the same rows plus the current dependency revision.

Add `dependency_command_create_rotates_revision_once`: create a valid user relationship using the current revision and assert the returned revision differs from the supplied one.

Add `dependency_command_delete_rotates_revision_once`: delete a user-owned relationship and assert exactly one revision rotation.

Add `dependency_command_stale_create_revision_conflicts_without_store_mutation`: submit an obsolete revision and assert code `conflict`, unchanged Store contents and unchanged registry revision.

Add `dependency_command_stale_delete_revision_conflicts_without_store_mutation`: same for deletion.

Add `dependency_command_second_mutation_is_rejected_while_permit_active`: admit one mutation permit, attempt another mutation before completing the first and assert `conflict` without entering Store.

Add `dependency_command_failed_store_mutation_clears_permit_without_rotating_revision`: force Store create/delete failure after admission, assert permit is cleared, revision unchanged and a subsequent valid mutation may proceed.

Add `dependency_command_successful_mutation_completes_matching_permit_only`: attempt to complete with the wrong mutation id and assert registry refuses the completion without rotating revision.

Add `dependency_command_duplicate_maps_to_stable_error`: Store duplicate maps to code `dependency_duplicate`.

Add `dependency_command_self_reference_maps_to_stable_error`: self-reference maps to `dependency_self_reference`.

Add `dependency_command_invalid_endpoint_maps_to_stable_error`: missing capability maps to `dependency_invalid`.

Add `dependency_command_scanner_owned_delete_is_protected`: deletion of scanner-owned relation maps to `dependency_not_user_owned` and the relationship remains.

Add `dependency_command_not_found_maps_to_stable_error`: unknown relationship id maps to `not_found`.

Add `dependency_command_invalid_uuid_is_invalid_request`: malformed request UUID returns `invalid_request` before Store mutation or revision rotation.

Add `dependency_command_storage_details_are_not_exposed`: inject a Store failure containing a fake SQLite path and token and assert neither appears in `DependencyCommandError.message`.

Add `dependency_command_revisions_are_independent_from_scan_and_health`: rotate dependency revision and assert scan/health revisions are unchanged; rotate scan/health and assert dependency revision is unchanged.

Add `dependency_command_list_and_mutation_lock_order_does_not_deadlock`: use synchronization barriers to race a dependency list with a mutation and assert both finish within a bounded test timeout without holding Store and registry simultaneously.

- [ ] **Step 2: Run dependency command tests and verify RED**

Run: `cargo test -p control-center-desktop dependency_command_ --locked`

Expected: FAIL because `dependency_commands.rs` and `DependencyRegistry` do not exist.

- [ ] **Step 3: Implement DependencyRegistry permit semantics**

Implement an opaque workspace revision plus `in_flight: Option<Uuid>`. Admission validates the supplied revision and absence of an existing permit, allocates a fresh mutation id and stores it.

Implement `complete_success(mutation_id)` to require the exact active permit, rotate revision once and clear the permit. Implement `complete_failure(mutation_id)` to require the exact active permit, clear it and preserve revision. Do not expose mutation ids over IPC.

- [ ] **Step 4: Implement list/create/delete commands**

For create/delete, parse request ids first, admit a registry permit, release the registry lock, call the Store mutation, release Store and then complete the same permit as success/failure.

After successful mutation, obtain the authoritative dependency list from Store only after no registry guard is held, then return that list with the new revision.

If authoritative list retrieval fails after the mutation committed, return `storage_integrity` without reverting the already committed relationship or lying about the revision. The next `list_dependencies` call is the recovery path.

- [ ] **Step 5: Register dependency commands and bootstrap revision**

Add `mod dependency_commands;`, initialize `DependencyRegistry`, register all three dependency commands and expose `dependencyRevision` from bootstrap. Preserve scan and health bootstrap revisions independently.

Map dependency Store errors through an explicit match. No command path may return `error.to_string()` for raw storage failures.

- [ ] **Step 6: Run dependency command tests and verify GREEN**

Run: `cargo test -p control-center-desktop dependency_command_ --locked`

Expected: all revision, permit, mapping, ownership and deadlock tests PASS.

- [ ] **Step 7: Run full desktop regression tests**

Run: `cargo test -p control-center-desktop --locked`

Expected: PASS with existing scan and new health/dependency command tests.

- [ ] **Step 8: Commit Task 8**

Run: `git add apps/desktop/src-tauri/src/dependency_commands.rs apps/desktop/src-tauri/src/lib.rs`

Run: `git commit -m "feat: expose dependency mutations"`

Expected: one focused desktop commit containing dependency IPC, optimistic revisions and user-owned mutation protection.

---

### Task 9: Frontend health contracts, API adapter and run state

**Files:**
- Modify: `apps/frontend/src/model.ts`
- Modify: `apps/frontend/src/api.ts`
- Create: `apps/frontend/src/useHealth.ts`
- Modify: `apps/frontend/src/App.test.tsx`
- Modify: `apps/frontend/src/App.desktop.test.tsx`

**Frontend model contracts:**

Add TypeScript types matching the Rust camelCase wire format exactly:

```ts
export type HealthState = "green" | "yellow" | "red" | "grey" | "blue";

export type HealthCheckType =
  | "path_exists"
  | "file_exists"
  | "config_present"
  | "config_parses"
  | "process_running"
  | "windows_service_state"
  | "loopback_port_reachable";

export type HealthRunState = "running" | "cancelling" | "cancelled" | "completed";
export type HealthRunScope = "installation" | "all";

export interface CanonicalCapability {
  id: string;
  canonicalName: string;
  stableIdentity: string | null;
  createdAt: string;
  updatedAt: string;
  revision: string;
}

export interface InstallationInstance {
  id: string;
  capabilityId: string;
  sourceDiscoveryId: string;
  kind: string;
  displayName: string;
  host: string;
  installationState: string;
  registrationState: string;
  enablementState: string;
  runtimeState: string;
  connectionState: string;
  authenticationState: string;
  aggregateHealth: HealthState;
  createdAt: string;
  updatedAt: string;
}

export interface HealthResult {
  id: string;
  runId: string;
  installationId: string;
  checkDefinitionId: string;
  checkType: HealthCheckType;
  required: boolean;
  state: HealthState;
  resultCode: string;
  redactedEvidence: string;
  durationMs: number;
  observedAt: string;
}

export interface HealthCheckSnapshot {
  definition: HealthCheckDefinition;
  latestResult: HealthResult | null;
  stale: boolean;
}

export interface InstallationHealthSnapshot {
  installation: InstallationInstance;
  capability: CanonicalCapability;
  checks: HealthCheckSnapshot[];
  effectiveHealth: HealthState;
  lastCheckedAt: string | null;
  warningCount: number;
  failureCount: number;
}

export interface HealthSummary {
  capabilityCount: number;
  installationCount: number;
  pendingDiscoveryCount: number;
  greenCount: number;
  yellowCount: number;
  redCount: number;
  greyCount: number;
  blueCount: number;
}

export interface HealthWorkspaceSnapshot {
  summary: HealthSummary;
  installations: InstallationHealthSnapshot[];
  revision: string;
}

export interface InstallationHealthDetail {
  snapshot: InstallationHealthSnapshot;
  history: HealthResult[];
}

export interface HealthRunHandle {
  runId: string;
  scope: HealthRunScope;
  state: HealthRunState;
  revision: string;
  startedAt: string;
  requestedInstallationCount: number;
  scheduledInstallationCount: number;
  skippedActiveInstallationCount: number;
}

export type HealthEvent =
  | { kind: "started"; runId: string; scope: HealthRunScope; requestedInstallationCount: number }
  | { kind: "check_completed"; runId: string; result: HealthResult }
  | { kind: "installation_updated"; runId: string; installationId: string; aggregateHealth: HealthState }
  | { kind: "cancelled"; runId: string; completedInstallationCount: number }
  | { kind: "completed"; runId: string; completedInstallationCount: number };
```

Keep the full `HealthCheckDefinition` and tagged `HealthTarget` TypeScript shape aligned one-for-one with the Rust serde representation from Task 1. Do not simplify the target into arbitrary strings.

Update `BootstrapState` to contain `pending`, `capabilities`, `installations`, `scanRevision`, `healthRevision` and `dependencyRevision`. Remove frontend reliance on legacy `inventory: Discovery[]` once all call sites in this task compile. Pending discoveries remain Discovery objects.

**API adapter:**

Add functions in `api.ts`:

```ts
export async function listHealthSummary(): Promise<HealthWorkspaceSnapshot>;
export async function listInstallationHealth(installationId: string): Promise<InstallationHealthDetail>;
export async function runHealthChecks(installationId: string, revision: string): Promise<HealthRunHandle>;
export async function runAllHealthChecks(revision: string): Promise<HealthRunHandle>;
export async function cancelHealthRun(runId: string, revision: string): Promise<HealthRunStateResponse>;
export async function listenHealthEvents(listener: (event: HealthEvent) => void): Promise<UnlistenFn>;
```

Desktop mode invokes exactly `list_health_summary`, `list_installation_health`, `run_health_checks`, `run_all_health_checks` and `cancel_health_run`, and listens to `health:event`. Keep request objects camelCase and let Tauri serialize them.

Browser/demo mode must remain clearly synthetic. Seed reviewed demo installations as Grey/Not checked with no fabricated successful HealthResult. Health changes only after the user explicitly invokes Run or Check All. Never auto-run checks when navigating to Health, Inventory or Overview.

Browser demo run simulation may use short timers, but it must obey the same visible lifecycle: started, zero or more check_completed/installation_updated events, then completed or cancelled. Cancellation stops future synthetic checks and preserves already completed synthetic results.

---

Add the remaining frontend wire types explicitly:

```ts
export interface HealthRunStateResponse {
  runId: string;
  state: HealthRunState;
  revision: string;
}

export type ConfigFormat = "json";
export type HealthExpectedCondition = "exists" | "parses" | "running" | "reachable";

export type HealthTarget =
  | { kind: "path"; path: string }
  | { kind: "config"; path: string; format: ConfigFormat }
  | { kind: "process_executable"; path: string }
  | { kind: "windows_service"; serviceName: string }
  | { kind: "loopback_socket"; host: string; port: number };

export interface HealthCheckDefinition {
  id: string;
  installationId: string;
  checkType: HealthCheckType;
  required: boolean;
  expectedCondition: HealthExpectedCondition;
  timeoutMs: number;
  staleAfterSeconds: number | null;
  target: HealthTarget;
  createdAt: string;
  updatedAt: string;
}
```

If Task 1 Rust serde chooses a different tag/content representation, update both Rust and TypeScript together before implementation so the wire test is exact. Do not introduce a frontend-only target representation.

**`useHealth` state contract:**

`useHealth.ts` owns only frontend health orchestration state. It does not calculate authoritative health from events. Keep:
- current `HealthWorkspaceSnapshot`;
- selected `InstallationHealthDetail | null`;
- active run handles keyed by `runId`;
- installation-to-active-run lookup;
- loading/error state;
- one installed `health:event` listener;
- buffered early events received between listener registration and run-handle return.

Expose this exact interface:

```ts
export interface UseHealthValue {
  workspace: HealthWorkspaceSnapshot | null;
  selected: InstallationHealthDetail | null;
  activeRuns: Record<string, HealthRunHandle>;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
  selectInstallation(installationId: string): Promise<void>;
  runInstallation(installationId: string): Promise<void>;
  runAll(): Promise<void>;
  cancelRun(runId: string): Promise<void>;
}
```

Run ordering follows the existing M6 listener-before-invoke protection: ensure the health listener is installed before invoking `run_health_checks` or `run_all_health_checks`. Buffer events for an unknown run id until the returned `HealthRunHandle` is registered, then replay them in arrival order.

Event handling rules:
- `started`: mark/display the run as active only. Do not invent health results.
- `check_completed`: may update visible progress/history optimistically using the persisted result carried by the event, but never derive the final workspace aggregate solely in the browser.
- `installation_updated`: may update the visible aggregate for that installation as a temporary event-driven projection.
- `cancelled` or `completed`: remove terminal run from active ownership, then call `listHealthSummary()` and, if the terminal run affected the selected installation, call `listInstallationHealth()` so SQLite-backed state replaces temporary event state.
- A terminal refresh response is authoritative over buffered/event-derived state.
- Navigation does not call `runInstallation` or `runAll`.
- Unmount removes the listener and clears browser-demo timers.

Conflict recovery: when run start returns code `conflict`, refresh `listHealthSummary()` to obtain the latest workspace revision and show a recoverable message. Do not automatically retry and unexpectedly start checks. When cancellation returns conflict, refresh current health state and require another explicit user cancel action if the run is still active.

`health_run_active` is a recoverable UI state, not a fatal application error. Show that the installation already has an active health run and leave existing active state intact.

---

- [ ] **Step 1: Add failing frontend health adapter/state tests**

Add focused tests in `App.desktop.test.tsx` or a new colocated `useHealth.test.tsx` if extracting the hook makes the tests clearer. Use mocked API functions and a controllable health-event listener.

Required tests:
- `health_frontend_bootstrap_uses_capabilities_installations_and_independent_revisions`: no legacy `inventory` dependency remains in rendered application state.
- `health_frontend_navigation_does_not_start_checks`: visit Health, Inventory and Overview and assert neither run API was called.
- `health_frontend_initial_reviewed_installation_is_grey_not_green`: a reviewed installation with no results renders Not checked/Grey.
- `health_frontend_listener_is_ready_before_single_run_invoke`: make the mock emit `started` synchronously during invoke and assert it is observed.
- `health_frontend_listener_is_ready_before_check_all_invoke`: same for Check All.
- `health_frontend_buffers_event_before_handle_registration`: emit `check_completed` before the run promise resolves and assert the event is replayed after handle registration rather than lost.
- `health_frontend_terminal_completed_refreshes_backend_authority`: emit Completed and assert `listHealthSummary` is called after the event.
- `health_frontend_terminal_cancelled_refreshes_backend_authority`: same for Cancelled.
- `health_frontend_selected_installation_refreshes_after_terminal`: when the selected installation participated, assert detail is re-fetched.
- `health_frontend_completed_results_survive_cancel`: emit one completed result, then Cancelled and return that persisted result from backend refresh; assert it remains visible.
- `health_frontend_cancel_uses_current_run_revision`: assert `cancelHealthRun` receives the active handle revision.
- `health_frontend_cancel_response_updates_run_revision`: Cancelling response replaces the run revision used by subsequent actions.
- `health_frontend_run_conflict_refreshes_without_auto_retry`: conflict triggers summary refresh and exactly one run invocation.
- `health_frontend_cancel_conflict_refreshes_without_auto_retry`: conflict refreshes and does not send a second cancellation automatically.
- `health_frontend_active_error_preserves_existing_run`: `health_run_active` does not erase or duplicate active-run state.
- `health_frontend_unmount_removes_health_listener`: assert unlisten executes exactly once.
- `health_frontend_browser_demo_requires_explicit_run`: advance fake timers after navigation alone and assert no synthetic result appears.
- `health_frontend_browser_demo_cancel_stops_future_results`: start demo run, allow one result, cancel, advance timers and assert no additional result appears.

- [ ] **Step 2: Run frontend health tests and verify RED**

Run: `pnpm --filter @ai-tool-control-center/frontend test -- --runInBand`

If the existing Vitest configuration does not support `--runInBand`, use the repository's existing `pnpm --filter @ai-tool-control-center/frontend test` command instead and record the actual command used.

Expected: FAIL because the M7 frontend models, API adapter and `useHealth` orchestration do not exist.

- [ ] **Step 3: Implement exact frontend models and API adapter**

Update `model.ts` and `api.ts` with the M7 wire contracts. Migrate browser fixtures from reviewed `Discovery[]` inventory to explicit canonical capabilities/installations. Keep pending discoveries unchanged.

Desktop API functions must contain no health policy. They only invoke/listen and normalize command errors into the existing frontend error handling style.

Browser/demo mode may return synthetic local outcomes only after explicit Run/Check All and must clearly retain demo mode labeling already used by the application. Do not imply synthetic outcomes came from the machine.

- [ ] **Step 4: Implement `useHealth` with listener-before-invoke buffering**

Install one listener lazily before the first health run and keep it for the hook lifetime. Use a `Map<string, HealthEvent[]>` for early events whose run handles are not registered yet. After a start command resolves, register its handle, drain that run's buffer in original order and discard the buffer.

Keep terminal refresh sequencing asynchronous but deterministic. Protect against state updates after unmount. Avoid duplicate listeners when `refresh`, `runInstallation` and `runAll` are called repeatedly.

- [ ] **Step 5: Update existing frontend fixtures without changing screen design yet**

Update `App.test.tsx` and `App.desktop.test.tsx` bootstrap mocks to provide `capabilities`, `installations`, `healthRevision` and `dependencyRevision`. Preserve all existing M6 scan lifecycle tests and review-gate behavior.

When a discovery is imported in frontend tests, refresh authoritative bootstrap/canonical inventory rather than optimistically treating the Discovery itself as an InstallationInstance.

- [ ] **Step 6: Run frontend health tests and verify GREEN**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: new health orchestration tests and all existing M6 frontend tests PASS.

- [ ] **Step 7: Run frontend type/build gate**

Run: `pnpm --filter @ai-tool-control-center/frontend build`

Expected: PASS with no TypeScript wire-shape errors.

- [ ] **Step 8: Commit Task 9**

Run: `git add apps/frontend/src/model.ts apps/frontend/src/api.ts apps/frontend/src/useHealth.ts apps/frontend/src/App.test.tsx apps/frontend/src/App.desktop.test.tsx`

Run: `git commit -m "feat: add frontend health orchestration"`

Expected: one focused frontend commit containing M7 typed health contracts, API adapters, explicit browser-demo runs and event-driven state with authoritative terminal refresh.

---

### Task 10: Health workspace, Inventory health and Overview metrics

**Files:**
- Create: `apps/frontend/src/HealthPage.tsx`
- Modify: `apps/frontend/src/App.tsx`
- Modify: `apps/frontend/src/styles.css`
- Modify: `apps/frontend/src/App.test.tsx`
- Modify: `apps/frontend/src/App.desktop.test.tsx`

**UI responsibility boundary:**

`HealthPage.tsx` owns the Health workspace presentation and consumes `UseHealthValue`. `App.tsx` keeps navigation, bootstrap/review flow, Overview and Inventory composition. Do not move scan orchestration into HealthPage and do not duplicate health API calls outside `useHealth`.

The Health workspace must show reviewed installation instances only. Pending discoveries remain in the existing review workflow and are not presented as checked installations.

**Health workspace list:**
- Header with title `Health`, compact explanation that checks run only when requested and primary `Check All` action.
- When a Check All run is active, show progress using requested, scheduled and skipped counts plus a Cancel action for that run.
- Each installation row shows display name, canonical capability name, effective health state, last checked time, warning count, failure count and a `Run checks` action.
- A never-checked installation renders Grey with user-facing text `Not checked`.
- Running state may temporarily display `Checking` without changing the persisted health state.
- Blue must be labelled as setup/action required rather than failure.
- Yellow is warning/degraded/stale.
- Red is failed required check.
- Grey is unknown/not checked/insufficient evidence.
- Green is healthy only when backend aggregation reports Green.
- Do not infer Green from existence in inventory, discovery confidence or review status.

**Installation detail:**
Selecting an installation loads `listInstallationHealth` through `useHealth.selectInstallation`. Display:
- capability and installation names;
- current aggregate state;
- last checked timestamp;
- each configured check type;
- Required or Optional label;
- latest state;
- result code;
- observed time;
- duration;
- stale indicator;
- redacted evidence only;
- compact newest-first persisted history.

If there are no definitions, explain that no supported local health checks are configured and keep state Grey. Do not invent a generic success.

**Inventory integration:**

Replace the reviewed Inventory source from legacy `Discovery[]` to `InstallationInstance[]` joined with `CanonicalCapability` by `capabilityId`. Each inventory row shows:
- installation display name;
- canonical capability name;
- kind;
- host;
- aggregate/effective health;
- state fields already carried by InstallationInstance where useful;
- source-discovery evidence remains accessible through the existing reviewed-discovery linkage where the current UI already presents it, but evidence text must not be parsed to derive M7 health.

When current health projection is available, prefer `InstallationHealthSnapshot.effectiveHealth` over the persisted aggregate so TTL staleness is visible. Otherwise use the persisted installation aggregate as the bootstrap value.

**Overview integration:**

Remove hard-coded health failure and unknown-state metrics. Overview uses backend-derived counts:
- `Capabilities` = canonical capability count;
- `Installations` = installation instance count;
- `Pending review` = pending discovery count;
- `Healthy` = Green installation count;
- `Warnings` = Yellow installation count;
- `Failures` = Red installation count;
- `Unknown` = Grey installation count;
- `Setup required` = Blue installation count.

The five health-state counts must total the reviewed installation count, not capability count and not pending discoveries.

If the Health workspace has not yet been fetched after bootstrap, render bootstrap installation counts conservatively and trigger a normal read-only health summary refresh. A read-only refresh is allowed on page load/navigation; running checks is not. Never translate missing health data to Green.

**Accessibility and interaction:**
- Every Run, Check All and Cancel control is a real button with disabled/busy state.
- Status is conveyed by visible text, not color alone.
- Detail rows use semantic labels and readable timestamps.
- Preserve keyboard navigation and existing application focus behavior.
- Do not introduce a graph library or new frontend package in this task.

---

- [ ] **Step 1: Add failing Health workspace and integration tests**

Add tests with the `health_ui_` prefix in `App.test.tsx` and `App.desktop.test.tsx`. Prefer user-visible assertions through Testing Library rather than inspecting component internals.

Required tests:
- `health_ui_lists_reviewed_installations_not_pending_discoveries`: pending review rows do not appear in the Health installation list.
- `health_ui_never_checked_installation_displays_not_checked`: Grey installation with no latest result visibly says `Not checked`.
- `health_ui_does_not_treat_reviewed_as_healthy`: importing/reviewing an installation alone never renders Healthy/Green for that installation.
- `health_ui_check_all_invokes_explicit_run`: clicking `Check All` invokes `runAll` exactly once with no automatic invocation before the click.
- `health_ui_single_run_invokes_selected_installation`: clicking `Run checks` on one row invokes only that installation.
- `health_ui_active_check_all_shows_requested_scheduled_and_skipped_counts`: active handle counts are rendered without fabricating completed checks.
- `health_ui_cancel_control_cancels_active_run`: clicking Cancel passes the active run id to `cancelRun`.
- `health_ui_checking_is_temporary_not_persisted_health`: while a run is active the UI may say `Checking`, but the underlying visible last known state remains available and is not rewritten to Green.
- `health_ui_blue_is_setup_required_not_failure`: Blue renders setup/action-required wording and does not increment/display as a required failure.
- `health_ui_yellow_is_warning`: Yellow is visibly labelled as warning/degraded/stale.
- `health_ui_red_is_required_failure`: Red is visibly labelled as a failed required check.
- `health_ui_detail_loads_on_selection`: selecting an installation calls `selectInstallation` with its installation id and renders returned detail.
- `health_ui_detail_shows_required_optional_state_duration_and_time`: check rows expose required/optional status, result state, observed time and duration.
- `health_ui_detail_marks_stale_checks`: stale snapshot visibly indicates stale status.
- `health_ui_detail_shows_only_redacted_evidence_field`: rendered evidence comes from `redactedEvidence`; no target/config content is reconstructed client-side.
- `health_ui_no_definitions_remains_grey`: empty check definitions render the supported-checks explanation and never Healthy.
- `health_ui_history_is_newest_first`: persisted history returned newest-first stays in that order in the compact history view.

Add Inventory integration tests:
- `health_ui_inventory_uses_installation_instances`: reviewed inventory renders InstallationInstance display/canonical names rather than legacy Discovery rows.
- `health_ui_inventory_prefers_effective_health_projection`: when persisted aggregate is Green but current projection is Yellow because stale, Inventory displays Yellow.
- `health_ui_inventory_bootstrap_grey_is_not_promoted`: without a current health projection, a Grey bootstrap aggregate remains Grey.

Add Overview integration tests:
- `health_ui_overview_uses_backend_health_counts`: render a summary with distinct Green, Yellow, Red, Grey and Blue counts and assert each metric matches exactly.
- `health_ui_overview_counts_installations_separately_from_capabilities`: capability count and installation count may differ and both are displayed correctly.
- `health_ui_overview_pending_is_not_unknown_health`: pending discovery count is separate from Grey installation count.
- `health_ui_overview_health_counts_total_installations`: fixture health-state counts sum to installation count and the rendered values preserve that distinction.
- `health_ui_overview_removes_hardcoded_zero_failures`: a backend Red count greater than zero is rendered rather than the old constant zero.
- `health_ui_read_only_refresh_may_run_on_navigation_but_health_checks_do_not`: navigation may call `refresh`, but `runInstallation` and `runAll` remain untouched.

- [ ] **Step 2: Run Health UI tests and verify RED**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: FAIL because HealthPage and the M7 Inventory/Overview projections are not implemented.

- [ ] **Step 3: Implement `HealthPage.tsx`**

Build the Health list and detail view strictly from `UseHealthValue`. Keep state labels in one small mapping helper so Green/Yellow/Red/Grey/Blue wording remains consistent across list and detail.

Derive active installation ownership only from `activeRuns` and run scope metadata already held by `useHealth`. Do not create a second health-run state machine inside the page.

Show `Checking` only as transient activity text. Render the last authoritative/effective health label alongside it where needed so activity is never mistaken for a new persisted outcome.

- [ ] **Step 4: Replace legacy reviewed Inventory rendering**

In `App.tsx`, join `bootstrap.installations` to `bootstrap.capabilities` by `capabilityId`. Use the matching `HealthWorkspaceSnapshot.installations` projection when available to obtain effective TTL-aware health.

Do not parse discovery evidence, suggested names, executable strings or paths to derive capability grouping or health. Source discovery remains provenance only.

- [ ] **Step 5: Replace hard-coded Overview metrics**

Use `HealthWorkspaceSnapshot.summary` as the authoritative read model whenever loaded. During the brief bootstrap-only state, compute only conservative structural counts from bootstrap and keep reviewed health exactly as persisted, with missing health remaining Grey/unknown rather than Green.

Trigger `health.refresh()` as a read-only operation when the application needs the authoritative summary. Do not couple refresh to any run command.

- [ ] **Step 6: Add Health workspace styling and accessibility states**

Extend `styles.css` using existing application tokens and layout conventions. Add status pills/labels, progress treatment, detail rows and compact history without introducing a dependency.

Buttons must expose disabled/busy state where applicable. Use text labels for every status so color is supplemental only. Preserve responsive behavior of the existing shell.

- [ ] **Step 7: Run frontend tests and verify GREEN**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: all new Health/Inventory/Overview tests and all existing M6 frontend tests PASS.

- [ ] **Step 8: Run frontend lint and build gates**

Run: `pnpm --filter @ai-tool-control-center/frontend lint`

Expected: PASS.

Run: `pnpm --filter @ai-tool-control-center/frontend build`

Expected: PASS.

- [ ] **Step 9: Commit Task 10**

Run: `git add apps/frontend/src/HealthPage.tsx apps/frontend/src/App.tsx apps/frontend/src/styles.css apps/frontend/src/App.test.tsx apps/frontend/src/App.desktop.test.tsx`

Run: `git commit -m "feat: add inventory health workspace"`

Expected: one focused frontend commit containing Health UI plus authoritative Inventory and Overview health integration.

---

### Task 11: Dependencies workspace and user-owned relationship editor

**Files:**
- Modify: `apps/frontend/src/model.ts`
- Modify: `apps/frontend/src/api.ts`
- Create: `apps/frontend/src/DependenciesPage.tsx`
- Modify: `apps/frontend/src/App.tsx`
- Modify: `apps/frontend/src/styles.css`
- Modify: `apps/frontend/src/App.test.tsx`
- Modify: `apps/frontend/src/App.desktop.test.tsx`

**Frontend contracts:**

Add exact dependency wire types:

```ts
export type DependencyKind = "requires" | "depends_on";
export type DependencyOwner = "scanner" | "user";

export interface DependencyRelationship {
  id: string;
  sourceCapabilityId: string;
  targetCapabilityId: string;
  kind: DependencyKind;
  owner: DependencyOwner;
  provenance: string;
  evidenceSummary: string;
  createdAt: string;
  revision: string;
}

export interface DependencyWorkspaceSnapshot {
  relationships: DependencyRelationship[];
  revision: string;
}

export interface DependencyMutationResponse {
  relationships: DependencyRelationship[];
  revision: string;
}
```

Add API functions:

```ts
export async function listDependencies(): Promise<DependencyWorkspaceSnapshot>;
export async function createUserDependency(
  sourceCapabilityId: string,
  kind: DependencyKind,
  targetCapabilityId: string,
  revision: string,
): Promise<DependencyMutationResponse>;
export async function deleteUserDependency(
  relationshipId: string,
  revision: string,
): Promise<DependencyMutationResponse>;
```

Desktop mode invokes exactly `list_dependencies`, `create_user_dependency` and `delete_user_dependency`. Browser/demo mode keeps dependency state in memory and enforces the same visible self-reference, duplicate and scanner-read-only behavior.

**Dependencies workspace:**
- Primary view is a deterministic relationship list.
- Each row shows source capability name, relationship type, target capability name and owner.
- `requires` and `depends_on` remain distinct labels.
- Scanner-owned rows display provenance/read-only status and no delete control.
- User-owned rows expose Delete.
- An Add relationship form lets the user choose source capability, relationship type and target capability from reviewed canonical capabilities only.
- Prevent selecting the same source and target in the client for usability, but backend validation remains authoritative.
- Do not infer relationships from installation names, paths, executable names, discovery evidence or visual proximity.

**Lightweight graph:**
- Secondary to the list, render a simple CSS/SVG capability graph using only the relationships already returned by the backend.
- Nodes represent canonical capabilities, not installation instances.
- Edges represent only persisted `requires` and `depends_on` relationships.
- The graph is read-only in M7. Editing remains through the list/form controls.
- Do not add a graph dependency or force-directed physics engine.
- When there are no relationships, show an empty-state explanation rather than invented edges.

**Mutation behavior:**
- Keep the current dependency revision in page state, initialized from bootstrap and replaced by `listDependencies`/mutation responses.
- On successful create/delete, replace the local relationship array with the authoritative response and use its new revision.
- On `conflict`, call `listDependencies()` once, update the revision/list and show a recoverable message. Do not automatically replay the mutation.
- `dependency_duplicate`, `dependency_self_reference`, `dependency_invalid` and `dependency_not_user_owned` are user-visible recoverable errors.
- Failed mutation does not optimistically alter the relationship list.
- Navigation/read-only loading never creates or deletes relationships.

---

- [ ] **Step 1: Add failing Dependencies workspace tests**

Add tests with the `dependency_ui_` prefix in `App.test.tsx` and `App.desktop.test.tsx`.

Required tests:
- `dependency_ui_list_is_primary_authoritative_view`: render persisted relationships and assert each source, kind, target and owner is visible in the list.
- `dependency_ui_requires_and_depends_on_are_distinct`: fixtures using both relationship kinds render different labels and are not collapsed together.
- `dependency_ui_scanner_relationship_is_read_only`: scanner-owned row shows scanner/read-only provenance and exposes no Delete control.
- `dependency_ui_user_relationship_can_be_deleted`: user-owned row exposes Delete and invokes `deleteUserDependency` with relationship id plus current revision.
- `dependency_ui_add_form_uses_reviewed_capabilities_only`: source and target selectors contain canonical capabilities and never pending discoveries or installation ids.
- `dependency_ui_client_blocks_self_reference`: selecting the same source and target disables submission or shows a local validation message and does not call the create API.
- `dependency_ui_create_passes_current_revision`: successful Add invokes `createUserDependency` with selected source, kind, target and current dependency revision.
- `dependency_ui_create_success_replaces_authoritative_list_and_revision`: mutation response replaces local relationships and revision rather than appending an optimistic guessed row.
- `dependency_ui_delete_success_replaces_authoritative_list_and_revision`: same authoritative replacement behavior for Delete.
- `dependency_ui_failed_create_does_not_optimistically_add`: rejected mutation leaves the prior list unchanged.
- `dependency_ui_failed_delete_does_not_optimistically_remove`: rejected deletion leaves the relationship visible.
- `dependency_ui_conflict_refreshes_once_without_auto_replay`: create/delete conflict calls `listDependencies` once, updates revision/list and does not invoke the mutation a second time.
- `dependency_ui_duplicate_is_recoverable`: `dependency_duplicate` renders a user-facing message and preserves the existing list.
- `dependency_ui_self_reference_backend_error_is_recoverable`: backend `dependency_self_reference` remains recoverable even though client validation also exists.
- `dependency_ui_invalid_endpoint_is_recoverable`: `dependency_invalid` shows a recoverable message without corrupting local state.
- `dependency_ui_scanner_delete_error_preserves_row`: a defensive `dependency_not_user_owned` response preserves the scanner-owned relationship.
- `dependency_ui_navigation_only_reads_relationships`: opening Dependencies may call `listDependencies`, but never create/delete APIs.
- `dependency_ui_empty_state_invents_no_edges`: with zero relationships, list and graph show an empty-state explanation and no SVG relationship edge.
- `dependency_ui_graph_nodes_are_capabilities_not_installations`: multiple installations belonging to one capability produce one graph node for that capability.
- `dependency_ui_graph_edges_match_persisted_relationships_exactly`: N persisted relationships produce N rendered relationship edges with matching source/target/kind metadata; no additional heuristic edge appears.
- `dependency_ui_graph_is_read_only`: clicking/keyboard interacting with graph nodes or edges does not invoke create/delete APIs.
- `dependency_ui_browser_demo_enforces_scanner_read_only`: browser/demo mode does not permit deletion of a synthetic scanner-owned relationship.

- [ ] **Step 2: Run dependency UI tests and verify RED**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: FAIL because dependency frontend models/API and `DependenciesPage.tsx` are not implemented.

- [ ] **Step 3: Implement dependency frontend models and API adapter**

Add the exact Task 11 types to `model.ts` and the three dependency API functions to `api.ts`. Desktop calls only Tauri commands. Browser mode uses explicit in-memory relationships and revision rotation without inventing scanner relationships from names or paths.

Normalize dependency command errors into the existing frontend command-error shape while preserving stable `code`. Do not expose or synthesize backend evidence beyond the returned redacted `evidenceSummary`.

- [ ] **Step 4: Implement `DependenciesPage.tsx` list and editor**

Load `listDependencies()` as a read-only operation when the workspace is opened. Keep relationships and revision together in component state.

Render canonical capability names by joining relationship endpoint ids against `bootstrap.capabilities`. If an endpoint is unexpectedly missing, render a safe `Unknown capability` label with the id shortened for diagnosis rather than guessing a name.

Implement Add with controlled source/kind/target selects. Disable submit for missing endpoints or self-reference. On success replace both relationships and revision from the response. On failure preserve the current list.

Render Delete only for owner `user`. Scanner-owned rows remain visibly read-only even if malformed frontend state attempts to expose a handler.

- [ ] **Step 5: Implement conflict/error recovery**

On `conflict`, call `listDependencies()` exactly once and replace list/revision from that response. Show a message that the relationship data changed and the user should retry their action. Do not automatically replay create/delete.

Map duplicate, self-reference, invalid endpoint and protected scanner ownership to concise recoverable UI messages. Unknown/storage failures use the existing generic local-storage error treatment and do not reveal raw backend details.

- [ ] **Step 6: Implement lightweight read-only graph**

Use plain SVG/CSS only. Derive a unique node set from canonical capability ids referenced by persisted relationships. Lay nodes out deterministically, for example in a simple fixed grid ordered by capability name then id.

Render exactly one edge per persisted relationship. Include a small visible/accessible relationship label for `Requires` or `Depends on`. Do not infer additional edges, merge relationships or run force simulation.

The list remains the primary accessible representation. Mark decorative SVG geometry appropriately and preserve readable text equivalents in the list.

- [ ] **Step 7: Run dependency UI tests and verify GREEN**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: all dependency workspace tests plus existing M6/M7 frontend tests PASS.

- [ ] **Step 8: Run frontend lint and build gates**

Run: `pnpm --filter @ai-tool-control-center/frontend lint`

Expected: PASS.

Run: `pnpm --filter @ai-tool-control-center/frontend build`

Expected: PASS.

- [ ] **Step 9: Commit Task 11**

Run: `git add apps/frontend/src/model.ts apps/frontend/src/api.ts apps/frontend/src/DependenciesPage.tsx apps/frontend/src/App.tsx apps/frontend/src/styles.css apps/frontend/src/App.test.tsx apps/frontend/src/App.desktop.test.tsx`

Run: `git commit -m "feat: add dependency workspace"`

Expected: one focused frontend commit containing authoritative dependency list, user-owned editor, scanner protection and lightweight read-only graph.

---

### Task 12: End-to-end Health and Dependencies workflows

**Files:**
- Create: `apps/frontend/e2e/health-dependencies.spec.ts`
- Modify only if required by deterministic testability: `apps/frontend/src/api.ts`
- Preserve: `apps/frontend/e2e/scan-orchestration.spec.ts`

Use the real browser/demo application path used by the existing Playwright suite. Do not mock React components or bypass the public UI. Synthetic browser data must remain visibly demo data and must obey the same explicit-run rules as desktop mode.

The E2E suite must prove these user journeys:

1. **Never checked stays unknown until explicit action**
   - Open the application in browser/demo mode.
   - Navigate to Health.
   - Assert at least one reviewed demo installation is `Not checked`/Grey.
   - Wait beyond the synthetic health timer duration and assert no health result appears automatically.
   - Navigate through Inventory and Overview and assert navigation itself still does not start a run.

2. **Single installation health run**
   - Return to Health and click one installation's `Run checks`.
   - Observe transient Checking/running UI.
   - Wait for terminal completion.
   - Assert the installation now displays the result returned by the explicit demo run.
   - Open detail and assert check type, required/optional label, outcome, duration/time and redacted evidence are visible.
   - Assert Inventory reflects the same installation health projection.
   - Assert Overview counts update from the authoritative demo health snapshot rather than hard-coded values.

3. **Check All cancellation preserves settled work**
   - Start `Check All`.
   - Wait until at least one synthetic check/result visibly settles.
   - Click Cancel before the full run finishes.
   - Wait for Cancelled terminal UI.
   - Assert already completed visible result/history remains.
   - Wait beyond all remaining demo timers and assert no additional post-cancellation result appears.
   - Assert untouched installations retain their prior health rather than being fabricated as checked.

4. **Dependency list and user editor**
   - Navigate to Dependencies.
   - Assert the persisted/demo relationship list is the primary visible representation.
   - Create one user-owned relationship between two distinct reviewed capabilities using the form.
   - Assert source, relationship kind, target and User owner appear in the list.
   - Assert the secondary graph gains exactly the corresponding persisted edge.
   - Delete that user-owned relationship and assert both list row and graph edge disappear.

5. **Scanner relationship remains read-only**
   - With a browser/demo scanner-owned relationship fixture, assert it is visibly Scanner/read-only.
   - Assert no Delete control is available for that relationship.
   - Assert opening/navigating the graph does not mutate it.

6. **No heuristic graph edges**
   - Use demo capabilities/installations with similar names where no relationship exists.
   - Assert they do not gain an edge merely because names, paths or executable text resemble one another.
   - Assert rendered graph edge count equals the relationship-list count exactly.

Use accessible roles/names for Playwright locators wherever possible. Avoid selectors coupled to CSS layout or generated SVG coordinates. Add stable `data-testid` only when an accessible locator cannot distinguish repeated graph geometry.

Do not weaken or replace the existing scan orchestration E2E suite. M7 tests are additive.

---

- [ ] **Step 1: Write failing M7 Playwright scenarios**

Create `health-dependencies.spec.ts` with the six workflows above. Keep test data deterministic through the existing browser/demo adapter. If additional deterministic fixtures are needed, add explicit demo-only records in `api.ts`; never add production-only shortcuts or global test backdoors.

- [ ] **Step 2: Run M7 E2E test and verify RED**

Run: `pnpm --filter @ai-tool-control-center/frontend test:e2e -- health-dependencies.spec.ts`

If the repository Playwright script does not forward a filename argument, run the existing `pnpm --filter @ai-tool-control-center/frontend test:e2e` command and use the test title/filter mechanism already supported by the repository. Record the actual working command.

Expected: FAIL until all M7 screens and demo orchestration are wired together.

- [ ] **Step 3: Make only deterministic integration fixes required by the E2E flow**

Fix integration defects exposed by the tests without changing approved M7 semantics. Keep browser/demo health explicitly user-triggered, cancellation-safe and clearly synthetic.

Do not solve flaky tests by increasing arbitrary waits. Prefer observable UI state, fake/demo deterministic sequencing and Playwright assertions that wait for the intended state transition.

- [ ] **Step 4: Run M7 E2E test and verify GREEN**

Run the same working M7 E2E command from Step 2.

Expected: all Health/Dependencies workflows PASS.

- [ ] **Step 5: Run the complete frontend E2E suite**

Run: `pnpm --filter @ai-tool-control-center/frontend test:e2e`

Expected: existing M6 scan orchestration plus new M7 Health/Dependencies scenarios all PASS.

- [ ] **Step 6: Run frontend regression gates**

Run: `pnpm --filter @ai-tool-control-center/frontend test`

Expected: PASS.

Run: `pnpm --filter @ai-tool-control-center/frontend lint`

Expected: PASS.

Run: `pnpm --filter @ai-tool-control-center/frontend build`

Expected: PASS.

- [ ] **Step 7: Commit Task 12**

Run: `git add apps/frontend/e2e/health-dependencies.spec.ts apps/frontend/src/api.ts`

If `api.ts` required no deterministic demo adjustment, stage only the E2E file.

Run: `git commit -m "test: cover M7 health and dependency workflows"`

Expected: one focused commit containing end-to-end M7 acceptance coverage without weakening M6 tests.

---

### Task 13: Full M7 verification and acceptance audit

**Files:**
- Modify only if verification exposes a real defect: files already owned by Tasks 1-12
- Update: `docs/superpowers/plans/2026-08-22-m7-health-dependency-systems-implementation.md` only to record verified command/results during execution if the project workflow keeps execution notes in plans
- Do not add unrelated cleanup or M8/M9 work

This task is verification-first. Do not change code merely to make a metric look cleaner. If any command fails, stop, diagnose the actual cause with the systematic-debugging workflow, add/repair the smallest relevant test, then rerun the failed gate before continuing.

---

- [ ] **Step 1: Verify formatting and Rust static analysis**

Run: `cargo fmt --all -- --check`

Expected: exit 0.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`

Expected: exit 0 with zero warnings promoted to errors.

- [ ] **Step 2: Verify the complete Rust test suite**

Run: `cargo test --workspace --locked`

Expected: exit 0. Record the actual test counts from the command output. Do not estimate or fabricate counts.

This gate must include the M7 model, migration, review import, probes, aggregation, persistence, orchestration, dependency ownership, health command, dependency command, revision and deadlock regression tests plus all pre-existing M6 tests.

- [ ] **Step 3: Verify release-mode Rust compilation**

Run: `cargo build --workspace --release --locked`

Expected: exit 0.

- [ ] **Step 4: Verify bundled Python engine regressions**

Run: `uv run --project engine/python ruff check engine/python`

Expected: exit 0.

Run: `uv run --project engine/python mypy engine/python/src`

Expected: exit 0.

Run: `uv run --project engine/python python -m unittest discover -s engine/python/tests`

Expected: exit 0. Record the actual test count reported by unittest.

M7 must not regress Python scanner compatibility even though product-specific health adapters remain deferred to M9.

- [ ] **Step 5: Verify deterministic frontend dependency installation**

Run: `pnpm install --frozen-lockfile`

Expected: exit 0 with no lockfile mutation required.

- [ ] **Step 6: Verify frontend lint, unit/integration tests and production build**

Run: `pnpm lint`

Expected: exit 0.

Run: `pnpm test`

Expected: exit 0. Record actual suite/test counts.

Run: `pnpm build`

Expected: exit 0.

- [ ] **Step 7: Verify complete browser E2E coverage**

Run: `pnpm --filter @ai-tool-control-center/frontend test:e2e`

Expected: exit 0 for both the existing M6 scan orchestration scenarios and the new M7 Health/Dependencies workflows. Record actual Playwright test counts.

- [ ] **Step 8: Verify repository policy**

Run: `python scripts/repository_policy.py --root .`

Expected: exit 0. No repository-policy exception is added solely to permit M7 implementation.

- [ ] **Step 9: Verify Tauri release compilation**

Run the repository's existing Tauri no-bundle release-build command. Prefer: `pnpm tauri build --no-bundle` if that is the command already established by the repository scripts. If the root script differs, inspect `package.json`, use the existing equivalent and record the exact command actually run.

Expected: exit 0. This is the M7 acceptance proof that the Windows desktop shell still compiles in release mode. Do not claim installer/package verification from this command.

- [ ] **Step 10: Verify repository diff hygiene**

Run: `git diff --check`

Expected: exit 0 with no whitespace errors.

Run: `git status --short`

Expected before the final M7 commit: only intentional M7 files are modified/untracked. Investigate any unrelated file before staging it.

---

### M7 acceptance audit

After every verification command above passes, explicitly map evidence to all approved acceptance criteria. Do not mark an item complete from design intent alone.

1. Reviewed inventory is represented as canonical capabilities plus installation instances.
2. Existing reviewed inventory migrates transactionally.
3. New and migrated installations begin Grey/Not checked.
4. Health execution occurs only after explicit user action.
5. All seven approved generic typed local checks are implemented.
6. Every individual health check has a hard timeout.
7. Single-installation and Check All runs are cancellable.
8. Check All executes at most four installation workers concurrently.
9. Completed health results are persisted append-only and history remains available.
10. Green, Yellow, Red, Grey and Blue states are represented end-to-end.
11. Missing required evidence/check results can never produce Green.
12. Durable health state is persisted before corresponding frontend durable-state events.
13. Health workspace displays actual persisted/backend-projected results.
14. Inventory displays installation aggregate/effective health rather than discovery existence.
15. Overview displays actual installation health counts with capabilities/installations/pending distinguished.
16. Dependency kinds `requires` and `depends_on` both work end-to-end.
17. Scanner-proposed dependencies cross the review gate and require deterministic exact target resolution.
18. User-owned dependencies are editable while scanner-owned relationships are protected from user deletion/scanner overwrite.
19. Authoritative dependency list works independently of the secondary graph.
20. Complete Rust workspace test suite passes.
21. Complete frontend unit/integration test suite passes.
22. Complete frontend E2E suite passes.
23. Repository policy passes.
24. Tauri release compilation passes.

For criteria 1-19, cite the exact test name(s) or verified user-visible E2E scenario that prove each item in the execution report. For criteria 20-24, cite the exact command and exit status.

Also explicitly verify the safety/non-goals remain intact:
- no background or scheduled health checks;
- no automatic checks on launch, import or navigation;
- no non-loopback network probing;
- no DNS-based health probing;
- no config mutation or secret inspection/persistence;
- no process/service start, stop or repair actions;
- no paid/cloud/API health checks;
- no product-specific M9 adapter behavior;
- no heuristic capability merge;
- no heuristic dependency creation;
- no new graph library;
- no telemetry;
- no automatic background networking.

---

- [ ] **Step 11: Run final M7-specific focused regressions after any verification fixes**

If Steps 1-10 required code changes, rerun every directly affected focused test group first, then rerun the entire verification sequence from Step 1. A previously passing full-suite result is not valid after later code changes.

- [ ] **Step 12: Create the milestone implementation commit only after all gates pass**

Inspect staged changes before committing. Do not stage the implementation plan accidentally if project convention keeps plan approval separate from implementation commits. Do not include unrelated local files.

The approved milestone commit message is:

`feat: add inventory health and dependency views`

Run this commit only after Tasks 1-12 implementation commits/tests are complete and every Task 13 gate passes. Do not merge the branch.

- [ ] **Step 13: Produce the verification report**

Report:
- branch/worktree;
- final implementation commit SHA;
- each verification command actually run;
- exit status;
- actual Rust/Python/frontend/E2E test counts where the tools report them;
- acceptance criteria 1-24 with concrete evidence;
- any intentionally deferred M9 behavior;
- final `git status --short`;
- confirmation that main was not modified or merged.

Do not claim success for any command that was not executed successfully in the current implementation state.

---
