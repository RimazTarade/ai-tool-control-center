# Milestone 7 - Health and Dependency Systems Design

**Date:** 2026-08-22  
**Status:** Approved design  
**Milestone:** M7  
**Branch:** `feat/0.2-m7-health-dependency-systems`  
**Authority:** `docs/superpowers/specs/2026-07-21-ai-tool-control-center-open-source-windows-design.md`

## 1. Purpose

Milestone 7 introduces trustworthy health evaluation and first-class dependency relationships for reviewed inventory.

The milestone also completes the transition from reviewed `Discovery` objects being used directly as inventory into the intended two-level domain model:

- canonical capabilities;
- installation instances.

M7 must preserve the product's existing privacy and trust boundaries:

- health checks are explicit and on-demand;
- no automatic background health checks;
- no paid or destructive operations;
- no non-loopback network probes;
- no secret-value inspection;
- no fabricated healthy state;
- machine-derived relationships do not bypass review;
- user-authored relationships are never overwritten by scanners.

M7 remains separate from:

- M6 scan orchestration;
- M8 operational controls;
- M9 product-specific adapter behavior.

## 2. Locked product decisions

### 2.1 Health execution

Health checks run only after an explicit user action.

Supported triggers:

- run checks for one reviewed installation;
- check all reviewed installations.

No health check runs automatically at import, bootstrap, application launch, navigation, timer, or background schedule.

### 2.2 Initial health-check scope

M7 implements generic typed local checks only.

Initial check types:

- `path_exists`;
- `file_exists`;
- `config_present`;
- `config_parses`;
- `process_running`;
- `windows_service_state`;
- `loopback_port_reachable`.

Product-specific health logic remains deferred to later adapter work.

### 2.3 Canonical capabilities and installations

A canonical capability represents the underlying product or function.

An installation instance represents one concrete reviewed local presence or registration route.

Health belongs to installation instances.

Dependencies belong to canonical capabilities.

### 2.4 Dependency relationship scope

M7 actively supports only:

- `requires`;
- `depends_on`.

Other relationship types remain deferred.

### 2.5 Dedicated health model

Health uses a dedicated enum rather than reusing the generic observed-state model.

States:

- `green`;
- `yellow`;
- `red`;
- `grey`;
- `blue`.

### 2.6 Health history

Every executed health check produces an append-only persisted result.

The latest applicable result for each configured check contributes to the installation's current aggregate health.

### 2.7 Dependency ownership

Relationships are explicitly owned by either:

- `scanner`;
- `user`.

Scanner-owned relationships originate from reviewed machine evidence.

User-owned relationships originate from explicit user actions.

User-owned relationships must not be overwritten or silently deleted by scanner activity.

### 2.8 Canonical identity matching

Automatic grouping into one canonical capability is conservative.

Automatic grouping is allowed only when strong deterministic identity exists.

Weak name similarity, path similarity, executable similarity, or heuristic matches must never silently merge capabilities.

Ambiguous matches may be proposed to the user but remain separate until explicitly resolved.

## 3. Domain model

### 3.1 CanonicalCapability

Represents the underlying logical product or function.

Conceptual fields:

- `id`
- `canonical_name`
- `stable_identity`
- `created_at`
- `updated_at`
- `revision`

`stable_identity` is optional and may contain a trusted deterministic identity supplied by reviewed core or adapter evidence.

### 3.2 InstallationInstance

Represents one reviewed concrete installation or registration route.

Conceptual fields:

- `id`
- `capability_id`
- `source_discovery_id`
- `kind`
- `display_name`
- `host`
- `installation_state`
- `registration_state`
- `enablement_state`
- `runtime_state`
- `connection_state`
- `authentication_state`
- `aggregate_health`
- `created_at`
- `updated_at`
- `revision`

The installation keeps linkage to the reviewed discovery and evidence that created it.

### 3.3 HealthState

Health states are:

- `green`
- `yellow`
- `red`
- `grey`
- `blue`

Meanings:

- Green: every configured required check passed and no degradation warning applies.
- Yellow: all required checks passed, but an optional warning, configured staleness, or degradation applies.
- Red: at least one configured required check failed.
- Grey: required evidence is missing, unknown, insufficient, or no required health checks have been completed.
- Blue: a configured required condition needs explicit setup or user action.

### 3.4 HealthCheckDefinition

A health-check definition describes one safe typed check.

Conceptual fields:

- `id`
- `installation_id`
- `check_type`
- `required`
- `expected_condition`
- `timeout_ms`
- `stale_after_seconds`
- `target_metadata`
- `created_at`
- `updated_at`

`stale_after_seconds` is optional.

Generic M7 checks default to no automatic expiry.

### 3.5 HealthRun

Represents one user-triggered health execution.

Conceptual fields:

- `id`
- `scope`
- `state`
- `started_at`
- `finished_at`
- `requested_installation_count`
- `completed_installation_count`
- `revision`

Scope is either:

- single installation;
- all reviewed installations.

Lifecycle:

- `Running -> Completed`
- `Running -> Cancelling -> Cancelled`

Individual probe failures do not fail the entire batch.

### 3.6 HealthResult

Every completed check attempt creates one append-only row.

Conceptual fields:

- `id`
- `run_id`
- `installation_id`
- `check_definition_id`
- `check_type`
- `required`
- `state`
- `result_code`
- `redacted_evidence`
- `duration_ms`
- `observed_at`

Historic results are never rewritten when a check runs again.

### 3.7 DependencyRelationship

Represents one directed capability-level relationship.

Conceptual fields:

- `id`
- `source_capability_id`
- `target_capability_id`
- `kind`
- `owner`
- `provenance`
- `evidence_summary`
- `created_at`
- `revision`

M7 relationship kinds:

- `requires`
- `depends_on`

Ownership:

- `scanner`
- `user`

## 4. Health aggregation

Aggregate health is deterministic and explainable.

Evaluation order is:

1. required failure;
2. required user/setup action;
3. required unknown, missing, or insufficient evidence;
4. optional warning or configured staleness;
5. healthy.

Therefore:

1. If any required check is Red, aggregate health is Red.
2. Otherwise, if any required check is Blue, aggregate health is Blue.
3. Otherwise, if any required check is Grey, missing, unexecuted, or insufficient, aggregate health is Grey.
4. Otherwise, if every required check passed but any optional check warns or applicable evidence is stale, aggregate health is Yellow.
5. Otherwise, aggregate health is Green.

An optional failed or warning check must never independently make aggregate health Red.

A passing check must never make an installation Green while another configured required check lacks sufficient evidence.

If no health checks have run for an installation, its aggregate health is Grey.

## 5. Health-check safety boundary

All M7 health checks are:

- local;
- bounded;
- non-destructive;
- typed;
- explicitly initiated by the user.

They must never:

- mutate configuration;
- start or stop processes;
- start or stop Windows services;
- modify accounts;
- create external content;
- make paid calls;
- send inventory content over a network;
- inspect or persist secret values;
- perform non-loopback network requests;
- hydrate cloud-only files.

Loopback connectivity checks may only target validated loopback destinations.

Configuration parsing must use formats already safely supported by reviewed core code.

M7 must not add arbitrary product-specific configuration parsers merely for health checking.

## 6. Health orchestration

Health orchestration is a separate subsystem from scan orchestration.

M6 scan registries, lifecycle state, revisions, or mutable scan maps must not be reused as health-run state.

Shared implementation patterns may be reused where appropriate:

- cancellation tokens;
- bounded concurrency;
- timeout enforcement;
- persist-before-notify;
- structured terminal events.

### 6.1 Single-installation execution

Only one active health run may target a given installation at a time.

Checks execute independently.

Every check has a hard timeout.

A slow or failing check must not permanently block unrelated checks.

### 6.2 Check All execution

`Check all reviewed installations` processes at most four installations concurrently.

The concurrency limit is a core constant in M7 and is not user-configurable.

An installation already being checked is skipped or reported as already active rather than receiving duplicate concurrent work.

### 6.3 Cancellation

Both single-installation and Check All runs are cancellable.

Cancellation rules:

- completed persisted results remain valid;
- in-flight checks receive cancellation;
- no new check begins after cancellation wins;
- untouched installations retain their previous results;
- cancellation does not replace valid previous results with Grey;
- the health run settles as Cancelled.

### 6.4 Database locking

Filesystem, process, service, configuration, and socket I/O must occur without holding the SQLite or Store lock.

The database lock is held only for short persistence or query operations.

## 7. Persist-before-notify

SQLite remains authoritative.

For each health result:

1. the probe completes;
2. the result is sanitized;
3. the result is persisted;
4. aggregate health is recomputed and persisted;
5. the frontend event is emitted.

The frontend must never be told that a result exists when persistence failed.

Frontend events are notifications, not the source of truth.

After a terminal health event, the frontend refreshes authoritative health projections from the backend.

## 8. Inventory migration

M7 introduces a transactional migration from the existing imported inventory representation.

The migration:

1. reads existing imported inventory rows;
2. preserves reviewed discovery and evidence linkage;
3. creates canonical capability records;
4. creates installation-instance records;
5. performs conservative deterministic grouping only when strong identity exists;
6. initializes aggregate health to Grey;
7. creates no health history;
8. invents no dependency relationships.

Ambiguous imported rows remain separate capabilities.

If migration fails, the prior database state remains intact.

The migration must not leave a partially converted inventory.

## 9. Review and machine-derived dependencies

Machine-derived dependencies do not bypass the mandatory review boundary.

A discovery may propose an unambiguous relationship.

When the user imports that discovery, the reviewed relationship may become active if:

- the relationship type is allowed;
- source capability resolves deterministically;
- target capability resolves deterministically;
- the relationship is not a self-reference;
- the relationship is not a logical duplicate.

If the target is ambiguous or unresolved, no relationship is created.

Scanner-owned relationships remain read-only from the manual dependency editor.

## 10. User-authored dependencies

The Dependencies page provides a minimal dependency editor.

The user selects:

1. source reviewed capability;
2. relationship kind;
3. target reviewed capability.

Allowed relationship kinds:

- `requires`;
- `depends_on`.

Validation rejects:

- source equals target;
- missing capability;
- duplicate relationship;
- unsupported relationship kind.

Users may delete relationships they authored.

The user-delete command must reject scanner-owned relationships.

Scanner activity must never overwrite or silently remove user-authored relationships.

## 11. Health page

The Health workspace becomes functional.

It presents reviewed installation instances grouped or filterable by:

- Green;
- Yellow;
- Red;
- Grey;
- Blue.

Each installation row shows:

- installation display name;
- canonical capability;
- aggregate health;
- last checked timestamp;
- warning count;
- failure count;
- Run checks action.

The page provides:

- Run checks for one installation;
- Check all reviewed installations;
- health-run progress;
- cancellation;
- health detail;
- recent health history.

Health detail shows for each configured check:

- check type or name;
- required or optional;
- latest outcome;
- last checked time;
- duration;
- redacted evidence summary;
- stale indicator where applicable.

M7 requires only a compact recent history view, not full analytics.

## 12. Dependencies page

The Dependencies workspace provides:

- accessible relationship list as the primary view;
- lightweight graph as a secondary view;
- minimal user relationship editor.

Each relationship shows:

- source capability;
- relationship kind;
- target capability;
- ownership.

Scanner-owned relationships are read-only.

User-owned relationships expose a delete control.

The graph must not be the only way to consume dependency information.

## 13. Inventory and Overview integration

### 13.1 Inventory

Inventory rows represent reviewed installation instances and must display their persisted aggregate health state.

Newly imported and migrated installation instances begin as:

`Grey / Not checked`

Installation presence alone must never imply Green health.

Inventory health presentation must distinguish the approved five states:

- Green
- Yellow
- Red
- Grey
- Blue

The frontend reads authoritative health state from the backend rather than deriving health from discovery or runtime observations.

### 13.2 Overview

The existing hard-coded health count must be removed.

Overview health counts must be computed from persisted installation-instance health state.

Overview must clearly distinguish:

- canonical capabilities;
- installation instances;
- pending discoveries.

These counts represent different concepts and must not be collapsed into one total.

Health summaries must reflect installation instances because health belongs to a specific local installation rather than to the canonical capability as a whole.

Missing health evidence remains Grey and must never be counted as Green.

## 14. IPC boundary

M7 extends the existing secure Rust-to-React command boundary.

The conceptual command surface is:

- `list_health_summary()`
- `list_installation_health(installation_id)`
- `run_health_checks(installation_id, revision)`
- `run_all_health_checks(revision)`
- `cancel_health_run(run_id, revision)`
- `list_dependencies()`
- `create_user_dependency(source_capability_id, kind, target_capability_id, revision)`
- `delete_user_dependency(relationship_id, revision)`

Exact command names may be refined during implementation planning, but the responsibility boundary is fixed.

Rust owns:

- input validation;
- health execution;
- timeouts;
- cancellation;
- health aggregation;
- persistence;
- dependency validation;
- dependency mutation;
- revision conflict handling.

React requests operations and renders backend projections. It must not execute arbitrary local commands or independently derive authoritative health state.

Mutating M7 operations extend the existing opaque revision-token model.

If a request carries a stale revision, the backend returns a conflict and must not overwrite newer state.

IPC responses and errors must remain structured and redacted. Raw SQLite errors, secret values, unsafe evidence, or credential-bearing data must never cross the IPC boundary.

## 15. Health events

M7 introduces structured health events for frontend progress and terminal-state updates.

Conceptual events are:

- `health.started`
- `health.check_completed`
- `health.installation_updated`
- `health.cancelled`
- `health.completed`

Each health event carries the authoritative `run_id`.

Health result events must be lossless. A completed health result must not be silently dropped merely to reduce frontend traffic.

The frontend may display temporary execution state such as:

`Checking...`

but temporary UI state is not authoritative health state.

Persisted backend data remains the source of truth.

After terminal health events, the frontend refreshes the relevant backend projection rather than treating event payloads as a replacement for persisted state.

Events must contain only structured, redacted information required for UI updates.

They must never expose secret values, unsafe raw configuration contents, credentials, or unredacted sensitive evidence.

## 16. Error semantics

M7 distinguishes negative health observations from command execution errors.

A negative observed condition is normally persisted as health data.

Example:

Expected:

`Windows service is running`

Observed:

`Windows service is stopped`

This is a failed health check result, not an IPC command failure.

Command-level errors are reserved for conditions where the requested health or dependency operation itself cannot be completed correctly.

Stable M7 error codes may include:

- `health_run_active`
- `health_check_timeout`
- `health_check_cancelled`
- `health_check_unsupported`
- `dependency_invalid`
- `dependency_duplicate`
- `dependency_self_reference`
- `dependency_not_user_owned`

Existing storage, validation, and revision-conflict errors remain in use where applicable.

Timeout and cancellation must be represented consistently in persisted run/check state and IPC responses.

Errors crossing the IPC boundary must be structured and safe.

The application must never expose through IPC:

- raw SQLite errors;
- secret values;
- API keys;
- OAuth tokens;
- passwords;
- cookies;
- Authorization headers;
- private keys;
- credential-bearing connection strings;
- secret environment values;
- unsafe unredacted evidence.

## 17. Staleness

M7 uses per-check staleness rather than a universal health expiry.

Each `HealthCheckDefinition` may optionally define:

`stale_after_seconds`

If no staleness value is configured, the result does not become stale automatically.

Generic M7 checks default to no automatic expiry.

Future adapters may define check-specific TTL values when the underlying evidence meaningfully ages.

When a configured health result becomes stale:

- the historical result remains preserved;
- the system does not delete or rewrite the original observation;
- the aggregate installation health may become Yellow if all required checks otherwise passed;
- staleness alone must not produce Red;
- missing required evidence still follows Grey semantics.

A universal 24-hour, 7-day, or other global expiry is explicitly rejected.

Staleness is evaluated deterministically from:

- the check definition;
- its configured TTL, if any;
- the latest persisted result timestamp;
- the current local evaluation time.

The frontend may show a stale indicator, but authoritative staleness and aggregate-health calculation remain backend responsibilities.

## 18. Testing strategy

M7 implementation follows test-driven development.

Tests must prove the approved semantics rather than merely exercise code paths.

### 18.1 Health aggregation tests

Tests must prove:

- no health run produces Grey;
- one passing required check cannot produce Green when another required check is missing;
- any failed required check produces Red;
- required setup or explicit user action produces Blue when no required Red exists;
- missing, unknown, insufficient, or not-run required evidence produces Grey when no higher-priority state applies;
- an optional warning may produce Yellow;
- an optional warning cannot independently produce Red;
- Green occurs only when every configured required check passes;
- configured staleness may produce Yellow when all required checks otherwise pass;
- a check with no TTL does not become stale automatically.

Aggregation precedence must be tested explicitly as:

`Red -> Blue -> Grey -> Yellow -> Green`

### 18.2 Orchestration tests

Tests must prove:

- each check has an independent hard timeout;
- one timed-out check does not prevent unrelated installations from completing;
- cancellation preserves already-persisted completed results;
- cancellation prevents new checks from starting after cancellation wins;
- untouched installations retain their previous health state;
- Check All runs at no more than four installations concurrently;
- one slow check does not block unrelated installations;
- a duplicate active run for the same installation is rejected;
- health results persist before frontend notification;
- filesystem, process, service, configuration, and socket probe I/O occurs without holding the database or Store lock.

Concurrency tests should be deterministic where possible and must not rely on arbitrary sleeps as their primary correctness proof.

### 18.3 Migration tests

Tests must prove:

- existing imported inventory migrates successfully;
- reviewed discovery and evidence linkage is preserved;
- migrated installation instances begin Grey;
- migration creates no fabricated health history;
- migration invents no dependency relationships;
- strong deterministic identity may group installations under one canonical capability;
- ambiguous identity remains represented as separate capabilities;
- a migration failure rolls back completely without leaving partial canonical or installation state.

### 18.4 Dependency tests

Tests must prove:

- `requires` relationships persist correctly;
- `depends_on` relationships persist correctly;
- self-referential relationships are rejected;
- duplicate relationships are rejected;
- unresolved machine-derived relationships are not created;
- scanner-owned relationships require reviewed evidence;
- scanner-owned relationships require deterministically resolved source and target capabilities;
- user-authored relationships survive later scanner activity;
- users can delete user-owned relationships;
- user delete operations cannot remove scanner-owned relationships.

### 18.5 Frontend tests

Frontend tests must cover:

- Grey / Not checked initial health state;
- per-installation Run checks;
- Check all reviewed installations;
- progress rendering;
- cancellation;
- Green state rendering;
- Yellow state rendering;
- Red state rendering;
- Grey state rendering;
- Blue state rendering;
- Inventory aggregate health display;
- Overview real persisted health counts;
- distinction between canonical capabilities, installation instances, and pending discoveries;
- dependency direction;
- dependency ownership;
- user dependency creation;
- self-link rejection;
- duplicate relationship rejection;
- absence of delete controls for scanner-owned relationships.

Frontend tests must not substitute client-side assumptions for backend health semantics.

### 18.6 E2E regression

At least one E2E scenario must exercise a deliberately slow health check while the user navigates between:

- Health;
- Inventory;
- Dependencies.

The user must be able to cancel the active health run.

Results completed and persisted before cancellation must remain visible afterward.

The E2E flow must also prove that navigation does not implicitly trigger health checks.

The final M7 verification gate must include:

- Rust workspace tests;
- frontend unit tests;
- frontend E2E tests;
- repository policy checks;
- frontend production build where required by the existing verification workflow;
- Tauri release compilation.

## 19. M7 acceptance criteria

M7 is not complete until every acceptance requirement below is satisfied.

1. Reviewed inventory uses canonical capabilities and installation instances.

2. Existing imported inventory migrates transactionally.

3. New and migrated installation instances start Grey.

4. Health checks run only after an explicit user action.

5. Generic typed local health checks operate within the approved safety boundary.

6. Every health check has a hard timeout.

7. Single-installation and Check All health runs are cancellable.

8. Check All executes no more than four installation instances concurrently.

9. Health history is append-only and persisted.

10. Aggregate health follows the approved Green, Yellow, Red, Grey, and Blue semantics.

11. Incomplete required evidence can never produce Green.

12. Health results and aggregate state persist before frontend notification.

13. The Health page renders real persisted health state.

14. Inventory displays aggregate health for installation instances.

15. Overview uses real installation-instance health counts.

16. Dependencies support both `requires` and `depends_on`.

17. Machine-derived dependencies pass through the mandatory review gate.

18. User-authored dependencies are editable and protected from scanner overwrite.

19. Dependency information remains accessible through a relationship list independently of the graph.

20. Rust tests pass.

21. Frontend unit tests pass.

22. Frontend E2E tests pass.

23. Repository policy checks pass.

24. Tauri release compilation passes.

## 20. Explicitly deferred work

The following capabilities are explicitly outside M7 scope:

- background health checks;
- scheduled health checks;
- automatic checks on application launch;
- automatic checks on import;
- automatic checks on navigation;
- non-loopback health networking;
- product-specific adapter health logic;
- paid API calls;
- process start, stop, or restart actions;
- service start, stop, or restart actions;
- install actions;
- repair actions;
- update actions;
- uninstall actions;
- broad dependency relationship vocabulary beyond `requires` and `depends_on`;
- aggressive heuristic canonical-capability merging;
- full health analytics;
- adapter-specific typed health definitions beyond the generic M7 foundation.

These items belong to later milestones or future adapter-specific work.

M7 must not expand into operational controls from M8 or declarative adapter behavior from M9.

The existing non-blocking M6 performance follow-up concerning blocking metadata operations is also not part of M7 unless implementation work naturally touches that area.

## 21. Architectural outcome

M7 establishes a durable health and dependency foundation without weakening the application's local-first, review-first, and explicit-user-control guarantees.

After M7:

- reviewed inventory is modeled through canonical capabilities and concrete installation instances;
- health belongs to installation instances rather than being inferred from discovery presence;
- missing or insufficient health evidence remains Grey;
- Green is possible only when every configured required check passes;
- health execution is explicit, local, bounded, cancellable, and non-destructive;
- health history is persisted and append-only;
- dependency relationships belong to canonical capabilities;
- machine-derived dependencies remain subject to mandatory review;
- user-authored dependencies remain protected from scanner overwrite;
- frontend health and dependency views are projections of persisted backend state rather than independent sources of truth;
- existing imported inventory is preserved through transactional migration;
- Rust remains responsible for validation, local execution, aggregation, persistence, and mutation boundaries.

M7 deliberately provides generic health and relationship infrastructure rather than product-specific automation.

This creates the stable domain and persistence layer required for later milestones:

- M8 safe operational controls;
- M9 declarative adapter packs;
- M10 backup, restore, diagnostics, and privacy;
- M11 installer and portable distribution.

The resulting architecture continues to enforce the project's core principles:

- Windows first;
- local only;
- zero telemetry;
- explicit user control;
- mandatory review before inventory import;
- explainable evidence;
- typed and bounded local operations;
- strict redaction;
- no arbitrary adapter code;
- no automatic background networking.
