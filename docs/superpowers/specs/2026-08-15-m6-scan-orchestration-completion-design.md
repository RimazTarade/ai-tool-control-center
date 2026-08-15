# Milestone 6 Scan Orchestration Completion Design

**Date:** 2026-08-15
**Branch:** `feat/0.2-m6-scan-orchestration-completion`
**Status:** Approved for implementation planning

## Purpose

Complete Milestone 6 of the AI Tool Control Center Windows implementation plan by bringing scan orchestration into alignment with the master specification.

This slice completes the shared Quick/Deep Scan lifecycle, cooperative pause and resume, the generic scan command contract, Deep Scan filesystem traversal, frontend controls and end-to-end verification.

## Goals

- Replace `start_quick_scan` with `start_scan(ScanRequest)`.
- Support Quick Scan and Deep Scan through one shared coordinator.
- Support real cooperative pause, resume and cancellation for both scan modes.
- Add a dedicated `filesystem.deep` scanner for explicitly selected drives and folders.
- Raise Quick Scan bounded concurrency from three to four scanners.
- Limit Deep Scan to eight concurrent directory reads.
- Preserve independent scanner timeouts and failure isolation.
- Add missing scan lifecycle events required by the master contract.
- Keep navigation and the review queue usable while scans run.
- Add frontend E2E coverage with Playwright.

## Non-Goals

This slice does not implement health and dependency systems, operational controls, adapter packs, backup and restore, distribution packaging or broader release hardening.

It also does not hydrate cloud-only placeholders or allow automatic network scanning without explicit per-scan consent.

## Architecture

Quick and Deep scans share lifecycle, event delivery, settlement, cancellation, pause handling, persistence and terminal cleanup.

Quick Scan builds the existing eight jobs:

- `filesystem.quick`
- `windows.known_location`
- `windows.path`
- `windows.uninstall_registry`
- `windows.process`
- `windows.service`
- `windows.tcp`
- `python.config`

Deep Scan builds one dedicated `filesystem.deep` job over explicitly selected roots.

Quick Scan uses bounded concurrency of four scanner jobs.

Deep Scan uses a bounded work queue with at most eight concurrent directory reads.


## Scan Request Contract

The desktop command contract becomes:

- `start_scan(ScanRequest) -> ScanHandle`
- `pause_scan(scan_id) -> ScanState`
- `resume_scan(scan_id) -> ScanState`
- `cancel_scan(scan_id) -> ScanState`

Conceptually, `ScanRequest` contains a scan mode, selected roots, and scan options.

`ScanHandle` contains `scan_id`, public `scope`, lifecycle `state`, opaque `revision`, and `started_at`. `ScanState` contains `scan_id`, lifecycle `state`, and the current opaque `revision` returned after the command settles. The public and persisted scope is only `quick` or `deep`; raw selected filesystem paths are never stored in `scan_runs.scope` or exposed as scan scope.

Mutating scan commands use request envelopes carrying `scan_id` and the caller's current `revision` even where the master-plan shorthand writes only `pause_scan(scan_id)`, `resume_scan(scan_id)`, or `cancel_scan(scan_id)`.

### Quick Scan

The frontend does not provide filesystem roots for Quick Scan.

The Tauri boundary resolves the normal Windows application-data roots and bundled Python application root internally.

### Deep Scan

Deep Scan accepts only roots explicitly selected by the user through the native folder or drive picker.

Deep Scan options include:

- `follow_reparse_points`, default `false`
- explicit per-run network consent where required

## Event Contract

The application continues to use a single Tauri `scan:event` channel.

The master-plan event names remain `scan.started`, `scan.progress`, `scan.discovery`, `scan.scanner_failed`, `scan.paused`, `scan.resumed`, `scan.cancelled`, `scan.completed`, and coordinator-level `scan.failed`. All messages travel over the single Tauri `scan:event` channel. On that channel the serialized `kind` discriminator uses the corresponding unprefixed snake-case values such as `started`, `progress`, `discovery`, `scanner_failed`, `paused`, `resumed`, `cancelled`, `completed`, and `failed`.

Conceptual event names:

- `scan.started`
- `scan.progress`
- `scan.discovery`
- `scan.scanner_failed`
- `scan.paused`
- `scan.resumed`
- `scan.cancelled`
- `scan.completed`
- `scan.failed`

`scan.started` carries public scope and scanner count. Quick Scan reports eight scanner jobs. Deep Scan reports one dedicated `filesystem.deep` scanner; its internal directory-read concurrency does not inflate the scanner count.

Progress must support scanner ID, visited count, optional total, and optional redacted current location.

Quick Scan may use indeterminate progress where a meaningful total does not exist.

Deep Scan emits a redacted current location and may remain indeterminate when total work cannot be known safely or cheaply.

Completion metadata should include visited count, discovery count, failure count, and duration.

The event queue is bounded. Progress may coalesce under backpressure, provided the frontend eventually receives a current progress state. Discoveries, scanner failures, and terminal events are never silently dropped. Discoveries are persisted before notification, scanner failures are persisted to `scan_errors` before notification, and terminal state is persisted to `scan_runs` before the terminal notification is emitted.

## Pause and Resume

Pause is cooperative. The system must never forcibly suspend worker threads or child processes.

### Quick Scan

Queued scanner jobs check the pause controller before acquiring the next scanner concurrency slot or beginning the next external scanner operation where a checkpoint is available.

`pause_scan` atomically closes the cooperative pause gate. `Paused` means no new checkpointed work may begin; it does not mean an already-running bounded operation has been forcibly suspended. An operation already in flight may finish and settle after the paused state is reported. No subsequent external scanner operation starts until `resume_scan` reopens the gate, and cancellation remains immediately available while paused.

### Deep Scan

Deep filesystem workers check the pause controller before acquiring the next directory or work item.

While paused:

- no new directory traversal work begins
- cancellation remains immediately available
- cancellation wakes paused workers without requiring resume first

Cancellation takes precedence over pause and resume races.


## Deep Scan Safety

The `filesystem.deep` scanner operates only on explicitly selected roots.

It must enforce:

- maximum eight concurrent directory reads
- default refusal to follow reparse points, junctions and symbolic links
- stable filesystem identity tracking when reparse traversal is explicitly enabled
- cycle prevention
- existing high-noise directory exclusions
- no automatic cloud-placeholder hydration
- per-scan confirmation for UNC or network roots
- bounded handling of access-denied, disappearing or unreadable paths
- path redaction before current-location data reaches the frontend

High-noise exclusions include categories such as:

- `.git`
- `node_modules`
- virtual environments
- package caches
- browser caches
- system restore data

Offline or recall-on-access cloud placeholders are skipped rather than hydrated.

## Network Roots

UNC or other network-backed roots are allowed only after explicit consent for the individual Deep Scan.

Consent is not persisted as a blanket global permission.

Cancelling the network-root confirmation leaves scan state unchanged.

## Reparse Points

Deep Scan exposes an advanced per-scan option:

`Follow symbolic links and junctions`

The option is off by default.

When enabled, traversal must track filesystem identity to prevent symbolic-link, junction or reparse cycles.

## Tauri Active Scan State

The active scan registry changes from storing only a `CancellationToken` to storing a richer scan handle containing:

- cancellation control
- pause control
- scan mode
- lifecycle state

This registry is the authoritative source for `pause_scan`, `resume_scan` and `cancel_scan`.

Every mutating scan request carries the current opaque revision token even where the shorthand command signatures omit it. A stale token returns the stable `conflict` error and performs no state change.

Starting a scan inserts a `scan_runs(id, scope, state, started_at, finished_at, failure_count)` record in the running state. Each scanner failure is written to `scan_errors(id, scan_id, scanner_id, code, redacted_message, observed_at)` before frontend notification. Completed, cancelled and failed settlement updates `scan_runs.state`, `finished_at` and `failure_count` before the corresponding terminal event is emitted.

Cancellation of an owned external scanner follows the master-plan process boundary: close child stdin, wait up to two seconds for cooperative exit, then close the owned Job Object and record `scanner_terminated` if forced termination was required.

Tauri window closure cancels every active scan and closes owned process trees before application teardown.

Terminal scans are removed exactly once.

## Lifecycle State

The conceptual lifecycle is:

```text
Running
  |
  +-- pause_scan() --> Paused
  |                     |
  |                     +-- resume_scan() --> Running
  |
  +-- cancel_scan() --> Cancelling --> Cancelled
```

State commands are idempotent where safe:

- pausing an already paused scan returns the paused state
- resuming an already running scan returns the running state

Unknown or terminal scan IDs return a stable error.


## Frontend UX

The existing `Run quick scan` button becomes one `Run scan` button.

It opens a compact scan dialog.

### Quick Scan

The dialog shows:

- Quick Scan selection
- a short description
- `Start scan`

### Deep Scan

The dialog shows:

- Deep Scan selection
- native folder or drive picker
- selected-root list
- remove controls
- advanced reparse-point toggle
- network-root warning and confirmation when applicable
- a note that cloud-only placeholders are skipped and not downloaded

## Running Scan Controls

The scan bar remains visible while the user navigates.

Running state:

```text
[Pause] [Cancel]
```

Paused state:

```text
[Resume] [Cancel]
```

Deep Scan additionally displays:

- `filesystem.deep`
- visited directory or item count
- redacted current location
- determinate progress only when a meaningful total is available

The Review Queue remains usable while discoveries stream in.

Discoveries are persisted before frontend notification.

## Error Handling

Scanner failures remain isolated.

A timeout or failure in one Quick Scan scanner must not prevent other scanner results from settling.

For Deep Scan, access denied, disappearing files, unreadable locations and other recoverable filesystem errors are handled as bounded local failures rather than aborting the whole scan.

Only unrecoverable coordinator or request-level failures produce `scan.failed`.

## Compatibility

This is a clean API migration.

`start_quick_scan` is removed after the frontend migrates to `start_scan(ScanRequest)`.

No duplicate legacy Quick Scan command is retained.

## Testing Strategy

Implementation follows test-driven development.

### Rust

Add tests for:

- Quick Scan concurrency of four
- cooperative pause checkpoints
- resume
- cancellation while paused
- cancellation of queued work
- independent scanner timeout isolation
- event backpressure
- deliberately slow scanner isolation
- Deep Scan concurrency of at most eight directory reads
- Deep Scan exclusions
- default reparse refusal
- cycle prevention when reparse traversal is enabled
- cloud-placeholder refusal
- network-root consent validation
- lifecycle event ordering and terminal settlement

### Tauri

Add focused coverage where practical for:

- request validation
- active scan state
- pause and resume command behavior
- cancellation
- terminal cleanup

### Frontend Unit and Component Tests

Use Vitest for:

- scan dialog behavior
- Quick versus Deep mode
- selected-root handling
- Pause/Resume/Cancel controls
- scan-state transitions
- failure recovery
- terminal-state cleanup

### End-to-End

Add Playwright and a `test:e2e` script.

The E2E suite must prove:

- navigation remains interactive during a deliberately slow scan
- review data remains visible during scanning
- pause prevents new scan work from progressing
- resume continues work
- cancellation terminates the scan
- a slow or failing scanner does not hide another scanner's results
- after terminal settlement, another scan can be started


## Verification Gates

Milestone 6 must pass:

```text
cargo test -p control-center-core scan --locked
pnpm --filter @ai-tool-control-center/frontend test --run
pnpm --filter @ai-tool-control-center/frontend test:e2e
```

Before merge, also run the relevant workspace formatting, lint, test, and build gates.

## Master-Plan Reconciliation

This implementation also corrects known deviations from the master plan:

- Quick Scan bounded concurrency changes from three to four.
- Real cooperative pause and resume replace the current demo-only frontend pause behavior.
- `scan.started`, `scan.paused`, and `scan.resumed` lifecycle support is added.
- Generic `start_scan(ScanRequest)` replaces the Quick-only desktop command.
- Deep Scan becomes a real `filesystem.deep` operation.
- Frontend E2E coverage becomes an explicit executable gate.
