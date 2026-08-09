# AI Tool Control Center open-source Windows implementation plan

**Plan date:** 2026-07-23  
**Initial application version:** 0.1.0  
**Target:** Windows 11 23H2+ x86-64  
**Authority:** `docs/superpowers/specs/2026-07-21-ai-tool-control-center-open-source-windows-design.md`

## Delivery rules

- Every milestone leaves the repository buildable and its focused checks green.
- Behavioral work starts with one focused failing test at the shared seam.
- The frontend never receives shell or unrestricted filesystem permissions.
- Community adapters contain data and typed operation identifiers only.
- Unknown evidence stays unknown; no scanner fabricates a healthy state.
- Generated build output, runtime databases, logs, caches, local adapters,
  Python runtimes, and WebView2 runtimes are not committed.
- Version 0.1.0 is a verified foundation, not a version 1.0 claim.

## Toolchain and dependency policy

- Rust 1.96.0, edition 2024, committed `Cargo.lock`.
- Tauri 2.11.x with compatible 2.11.x plugins and CLI.
- Node.js 24, pnpm 10.33.2, committed `pnpm-lock.yaml`.
- React 19.2.x, TypeScript 7.0.x, Vite 8.1.x.
- CPython 3.14 x86-64 embeddable distribution for release; Python 3.14 for
  development checks.
- Runtime Python scanner code uses the standard library. Ruff and mypy are
  development-only dependencies locked by `uv.lock`.
- Registries are public HTTPS sources. Git dependencies require immutable
  revisions; local, UNC, credential-bearing, and unapproved sources fail the
  repository policy check.

## Repository map and file responsibilities

### Root

- `Cargo.toml`: Rust workspace members and shared dependency versions.
- `Cargo.lock`: exact Rust dependency graph.
- `rust-toolchain.toml`: Rust version plus rustfmt and clippy.
- `pnpm-workspace.yaml`: JavaScript workspace definition.
- `pnpm-lock.yaml`: exact JavaScript dependency graph.
- `package.json`: root orchestration commands only.
- `.cargo/config.toml`: public crates.io sparse protocol and Windows target
  settings without credentials.
- `.npmrc`: public npm registry, lockfile, and safe package-script policy.
- `.gitignore`: local tools, runtimes, databases, logs, artifacts, caches,
  secrets, and editor state.
- `scripts/repository_policy.py`: secret-shaped value, private source, absolute
  path, generated state, lockfile source, and background-network policy scan.

### Rust core

- `engine/rust/control-center-core/Cargo.toml`: reusable core dependencies.
- `engine/rust/control-center-core/src/lib.rs`: public modules and stable API.
- `engine/rust/control-center-core/src/model.rs`: versioned domain and scanner
  contracts with separate status dimensions.
- `engine/rust/control-center-core/src/redaction.rs`: bounded redaction for
  logs, evidence, diagnostics, and action output.
- `engine/rust/control-center-core/src/action.rs`: typed action preview,
  inventory binding, validation, timeout, and execution policy.
- `engine/rust/control-center-core/src/adapter.rs`: strict typed adapter parsing
  and schema-version checks.
- `engine/rust/control-center-core/src/storage.rs`: SQLite migrations,
  transactions, review decisions, inventory queries, and consistent backup.
- `engine/rust/control-center-core/src/scan.rs`: scanner trait, coordinator,
  bounded event channel, cancellation, cooperative pause, timeouts, and
  per-scanner errors.
- `engine/rust/control-center-core/src/windows.rs`: Windows path, known-location,
  PATH executable, uninstall-registry, process, service, and TCP endpoint
  observation.
- `engine/rust/control-center-core/tests/`: one focused integration file per
  cross-module seam: storage/review, scan isolation, action security, adapter
  rejection, and Windows traversal.

### Python scanner

- `engine/python/pyproject.toml`: package metadata, Python version, Ruff, and
  mypy configuration.
- `engine/python/uv.lock`: exact development tool graph.
- `engine/python/src/ai_tool_control_scanner/__main__.py`: JSON Lines process
  entry point.
- `engine/python/src/ai_tool_control_scanner/protocol.py`: bounded versioned
  message decode/encode and redacted errors.
- `engine/python/src/ai_tool_control_scanner/scanners.py`: Claude, Codex, common
  MCP, Docker-output, and generic manifest parsing using supplied roots only.
- `engine/python/tests/`: protocol, cancellation, malformed input, timeout,
  redaction, and generic fixture tests.

### Desktop shell

- `apps/desktop/src-tauri/Cargo.toml`: thin Tauri crate.
- `apps/desktop/src-tauri/build.rs`: Tauri build integration.
- `apps/desktop/src-tauri/src/main.rs`: executable entry point.
- `apps/desktop/src-tauri/src/lib.rs`: app construction and plugin setup.
- `apps/desktop/src-tauri/src/commands.rs`: typed frontend commands mapped to
  core services.
- `apps/desktop/src-tauri/src/state.rs`: process-local scan registry and data
  directory ownership.
- `apps/desktop/src-tauri/capabilities/default.json`: local main-window core
  capability only; no shell or unrestricted filesystem grant.
- `apps/desktop/src-tauri/tauri.conf.json`: CSP, build paths, NSIS per-user
  installer, offline WebView2 mode, updater artifact settings, and resources.
- `apps/desktop/src-tauri/icons/`: generated first-party application icons.

### Frontend

- `apps/frontend/package.json`: React build, lint, test, and browser-test
  commands.
- `apps/frontend/index.html`: local application entry with no remote assets.
- `apps/frontend/src/main.tsx`: React bootstrap and error boundary.
- `apps/frontend/src/App.tsx`: route shell, onboarding, overview, inventory,
  review, health, dependencies, activity, adapter packs, backups, and settings.
- `apps/frontend/src/api.ts`: typed Tauri IPC plus explicit browser demo
  adapter used only by tests and labelled demo mode.
- `apps/frontend/src/model.ts`: frontend contract mirror.
- `apps/frontend/src/styles.css`: responsive, keyboard-visible, high-contrast
  application styling.
- `apps/frontend/src/App.test.tsx`: review, keyboard, scan progress, and
  navigation-during-scan component tests.
- `apps/frontend/e2e/navigation-during-scan.spec.ts`: deliberately slow scan
  browser regression.
- `apps/frontend/vite.config.ts`, `vitest.config.ts`, `eslint.config.js`, and
  `tsconfig.json`: strict build and test configuration.

### Adapters

- `adapters/schema/adapter.schema.json`: closed JSON Schema with typed probes
  and operations; no executable or shell fields.
- `adapters/schema/adapter.example.yaml`: generic, non-executable example.
- `adapters/built-in/core-tools.json`: reviewed built-in metadata.
- `adapters/community/README.md`: contribution gate.
- `examples/adapter-packs/example-local-runtime.yaml`: generic fixture.
- `examples/demo-catalog/catalog.json`: visibly labelled synthetic inventory.

### Packaging and CI

- `packaging/runtime-manifest.json`: exact CPython and WebView2 versions,
  official URLs, SHA-256 digests, architecture, and licences.
- `packaging/fetch-runtimes.ps1`: download, digest verification, extraction, and
  isolation-file validation.
- `packaging/portable/build-portable.ps1`: stage executable/resources/runtimes,
  create `Data`, normalize archive times, ZIP, list contents, and hash.
- `packaging/nsis/hooks.nsh`: preserve user data and optional desktop shortcut
  behavior without deleting application data on normal uninstall.
- `packaging/verify-artifacts.ps1`: unpack, enumerate, policy-scan, smoke, hash,
  and emit `BUILDINFO`.
- `.github/workflows/ci.yml`: locked build, unit, browser, policy, audit, and
  Tauri release-compile checks.
- `.github/workflows/release.yml`: protected tag build, runtime verification,
  NSIS/portable assembly, licence inventory, SBOM, checksums, signing hook,
  attestation, and draft artifact upload.

## Rust/Python/React interfaces

### Discovery contract

All scanners emit JSON objects matching `DiscoveryEnvelope`:

```json
{
  "protocol_version": 1,
  "scan_id": "UUID",
  "scanner_id": "stable-lowercase-id",
  "sequence": 1,
  "observed_at": "RFC3339 UTC",
  "discovery": {
    "fingerprint": "sha256 lowercase hex",
    "suggested_name": "string",
    "suggested_type": "unknown",
    "source": "filesystem|registry|process|service|port|config|cli",
    "confidence": "low|medium|high",
    "evidence": [{"kind": "path", "summary": "redacted string"}],
    "installation_state": "detected",
    "registration_state": "unknown",
    "enablement_state": "unknown",
    "runtime_state": "unknown",
    "connection_state": "unknown",
    "authentication_state": "unknown",
    "health_state": "unknown"
  }
}
```

Messages larger than 1 MiB, duplicate or decreasing sequence numbers, invalid
timestamps, unknown enum values, and protocol versions other than 1 are
rejected as a scanner error.

### Scan event contract

Rust emits:

- `scan.started`: scanner count and scope;
- `scan.progress`: completed units, optional total, redacted current location;
- `scan.discovery`: one validated discovery;
- `scan.scanner_failed`: scanner ID, stable error code, redacted message;
- `scan.paused` and `scan.resumed`;
- `scan.cancelled`;
- `scan.completed`: counts, duration, and failure count.

The event queue is bounded. Progress events may coalesce under backpressure;
discoveries and terminal events are persisted before notification and are not
silently dropped.

### Frontend command contract

- `bootstrap_state() -> BootstrapState`
- `start_scan(ScanRequest) -> ScanHandle`
- `pause_scan(scan_id) -> ScanState`
- `resume_scan(scan_id) -> ScanState`
- `cancel_scan(scan_id) -> ScanState`
- `list_pending(PageRequest) -> Page<Discovery>`
- `review_discoveries(ReviewBatch) -> ReviewResult`
- `list_inventory(InventoryQuery) -> Page<InstallationSummary>`
- `preview_action(ActionRequest) -> ActionPreview`
- `execute_confirmed_action(ConfirmedAction) -> ActionOutcome`
- `create_backup(BackupRequest) -> BackupSummary`
- `restore_backup_preview(path) -> RestorePreview`
- `restore_backup(ConfirmedRestore) -> RestoreOutcome`
- `create_diagnostic_preview() -> DiagnosticPreview`

Every mutating request includes an opaque revision token. Stale tokens return
`conflict` and never overwrite newer user data.

## Persistence schema

Migration 1 creates:

- `schema_migrations(version, applied_at)`;
- `capabilities(id, canonical_name, created_at, updated_at)`;
- `installations(id, capability_id, kind, host, display_name, executable,
  working_directory, installation_state, registration_state,
  enablement_state, runtime_state, connection_state, authentication_state,
  health_state, revision, created_at, updated_at)`;
- `discoveries(id, scan_id, fingerprint, payload_json, confidence,
  review_state, source_scanner, observed_at, revision)`;
- `evidence(id, discovery_id, installation_id, kind, summary, observed_at,
  scanner_owned)`;
- `relationships(id, source_capability_id, target_capability_id, kind,
  owner, created_at)`;
- `ignored_fingerprints(fingerprint, mode, created_at)`;
- `scan_runs(id, scope, state, started_at, finished_at, failure_count)`;
- `scan_errors(id, scan_id, scanner_id, code, redacted_message, observed_at)`;
- `action_history(id, installation_id, action_kind, preview_json, outcome,
  redacted_output, started_at, finished_at)`;
- `health_results(id, installation_id, check_kind, state, evidence_json,
  observed_at)`.

Foreign keys are enabled, user input is bound as parameters, and frequently
filtered fields have explicit indexes. User-authored catalogue JSON is
schema-versioned and atomically replaced; scans never write its fields.

## Cancellation, pause, and timeout

- One root cancellation token belongs to each scan.
- Each scanner gets a child token and a hard deadline.
- Quick scan uses bounded concurrency of four scanners.
- Deep filesystem workers use bounded concurrency of eight directory reads.
- Pause blocks only at checkpoints before acquiring the next directory or
  starting the next external scanner operation.
- Cancellation closes child stdin, waits two seconds, closes the Job Object,
  and records `scanner_terminated` if the process did not exit cooperatively.
- Tauri window closure cancels active scans and closes owned process trees.

## Errors and redaction

Stable public error codes are:

- `invalid_request`
- `permission_denied`
- `not_found`
- `conflict`
- `scanner_timeout`
- `scanner_cancelled`
- `scanner_protocol`
- `scanner_failed`
- `storage_migration`
- `storage_integrity`
- `adapter_invalid`
- `action_not_allowed`
- `action_timeout`
- `portable_not_writable`
- `internal`

Messages are redacted before persistence or IPC. Raw secrets, authorization
headers, cookies, private-key bodies, password-like fields, credential-bearing
URLs, and environment values never enter application logs or diagnostics.

## Milestone plan

### Milestone 0 — Public repository baseline and sanitization

1. Add the root governance files, source policy, ignore rules, toolchain pins,
   and generic examples.
2. Run `python scripts/repository_policy.py --root .`.
3. Expected: exit 0 with counts only and no secret values.
4. Run `git diff --check`.
5. Expected: exit 0.
6. Commit: `docs: vet open-source Windows platform specification`, then
   `chore: establish sanitized public repository baseline`.

### Milestone 1 — Tauri shell and secure IPC

1. Add a failing Rust test proving unknown command/action identifiers are
   rejected.
2. Add the thin Tauri shell, local-only capability, CSP, and typed bootstrap
   command.
3. Run `cargo test -p control-center-core action --locked`.
4. Expected: all focused tests pass.
5. Run `pnpm --filter @ai-tool-control-center/frontend build`.
6. Expected: strict TypeScript and production Vite build pass.
7. Run `cargo check -p ai-tool-control-center-desktop --locked`.
8. Expected: Tauri crate compiles.
9. Commit: `build: add Tauri Windows desktop workspace`.

### Milestone 2 — Domain model and persistence

1. Add failing storage tests for migration, separate states, rollback, stale
   revision rejection, and scanner/user ownership.
2. Implement migration 1 and repository methods.
3. Run `cargo test -p control-center-core storage --locked`.
4. Expected: all focused tests pass with temporary databases removed.
5. Commit: `feat: add domain model and local persistence`.

### Milestone 3 — Review queue and inventory

1. Add failing Rust review tests and React mandatory-review tests.
2. Implement import, edit/import, merge, ignore once, always ignore, keep
   unknown, bulk-safe decisions, search, and filters.
3. Run `cargo test -p control-center-core review --locked`.
4. Run `pnpm --filter @ai-tool-control-center/frontend test --run`.
5. Expected: discoveries remain pending until an explicit decision and
   ambiguous merge is rejected without a target.
6. Commit: `feat: add mandatory discovery review queue`.

### Milestone 4 — Native Windows discovery

1. Add Windows fixture tests for paths, exclusions, reparse refusal, registry
   views, access denied, process/service states, and TCP endpoint parsing.
2. Implement quick known-location, PATH, uninstall registry, process, service,
   and TCP scanners.
3. Run `cargo test -p control-center-core windows --locked`.
4. Expected: scanners return evidence or partial errors without panic.
5. Commit: `feat: add native Windows discovery framework`.

### Milestone 5 — Bundled Python scanner engine

1. Add failing protocol, malformed input, redaction, and generic config
   fixtures.
2. Implement the standard-library Python engine and Rust supervisor.
3. Run `uv run --project engine/python ruff check engine/python`.
4. Run `uv run --project engine/python mypy engine/python/src`.
5. Run `uv run --project engine/python python -m unittest discover -s engine/python/tests`.
6. Expected: lint, strict typing, and all tests pass.
7. Run the packaged interpreter smoke with `PYTHONHOME`, `PYTHONPATH`, and
   `PATH` cleared by `packaging/verify-artifacts.ps1`.
8. Expected: protocol handshake succeeds through the absolute bundled
   interpreter.
9. Commit: `feat: integrate isolated Python scanner engine`.

### Milestone 6 — Quick/deep scan orchestration

1. Add failing tests for timeout isolation, cancellation, cooperative pause,
   event backpressure, and a deliberately slow scanner.
2. Implement coordinator state and Tauri scan events.
3. Run `cargo test -p control-center-core scan --locked`.
4. Run `pnpm --filter @ai-tool-control-center/frontend test --run`.
5. Run `pnpm --filter @ai-tool-control-center/frontend test:e2e`.
6. Expected: navigation and review remain interactive during the slow scan;
   cancellation terminates it and other scanner results remain visible.
7. Commit: `feat: add cancellable scan orchestration`.

### Milestone 7 — Health and dependency systems

1. Add failing tests proving missing evidence stays unknown and green requires
   every configured required check to pass.
2. Implement typed local health checks and dependency relationships.
3. Run related Rust and frontend tests.
4. Expected: no synthetic healthy result and counts identify capability,
   instance, or pending-discovery scope.
5. Commit: `feat: add inventory health and dependency views`.

### Milestone 8 — Safe operational controls

1. Add failing injection, encoded-shell, stale-preview, wrong-inventory,
   timeout, output-bound, and unsigned-admin tests.
2. Implement open/copy/rescan and confirmed structured process/service actions.
3. Run `cargo test -p control-center-core action --locked`.
4. Expected: direct argument execution passes; shell interpolation,
   community-defined commands, and unsigned administrator requests fail.
5. Commit: `feat: add confirmed safe operational controls`.

### Milestone 9 — Declarative adapter packs

1. Add failing schema tests for unknown fields, raw commands, path traversal,
   unlabelled networking, duplicate IDs, and missing docs.
2. Implement strict JSON/YAML parsing and typed operations.
3. Run `cargo test -p control-center-core adapter --locked`.
4. Expected: valid examples pass and unsafe fixtures fail with stable codes.
5. Commit: `feat: add declarative adapter-pack schema`.

### Milestone 10 — Backup, restore, diagnostics, and privacy

1. Add failing backup checksum, live-WAL, schema, relationship, path
   traversal, rollback, and diagnostic-redaction tests.
2. Implement consistent backup, staged restore, preview, rollback, retention,
   and deletion.
3. Run `cargo test -p control-center-core backup --locked`.
4. Expected: invalid restore never replaces current state; diagnostic preview
   contains no fixture secrets or usernames.
5. Commit: `feat: add backup and diagnostic workflows`.

### Milestone 11 — NSIS and portable distribution

1. Verify runtime-manifest downloads and digests.
2. Build the frontend and Tauri release binary.
3. Run `pnpm tauri build --bundles nsis`.
4. Expected: per-user NSIS installer with offline WebView2 mode.
5. Run `pwsh packaging/portable/build-portable.ps1 -Configuration release`.
6. Expected: versioned portable ZIP with adjacent `Data`, fixed WebView2,
   private CPython, manifest, and SHA-256.
7. Run `pwsh packaging/verify-artifacts.ps1`.
8. Expected: both artifacts unpack, pass policy and private-path scans, and
   start with developer runtimes removed from `PATH`.
9. Commit: `build: add NSIS and portable packaging`.

### Milestone 12 — CI, documentation, and release hardening

1. Add pinned Actions, audits, licence inventory, SBOM, checksums, signing
   hook, build information, and artifact attestation.
2. Add getting-started, user, adapter, architecture, security, troubleshooting,
   support, governance, and release documentation.
3. Run the complete local verification commands below.
4. Expected: all locally executable gates pass and clean-desktop-only gates
   remain explicitly pending until run on the controlled VM.
5. Commit: `ci: add Windows verification and release workflows`, then
   `docs: add open-source guides and contribution policies`.

## Complete local verification

Run from repository root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
uv run --project engine/python ruff check engine/python
uv run --project engine/python mypy engine/python/src
uv run --project engine/python python -m unittest discover -s engine/python/tests
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm build
pnpm --filter @ai-tool-control-center/frontend test:e2e
python scripts/repository_policy.py --root .
pwsh packaging/verify-artifacts.ps1
git diff --check
```

Every command must exit 0. Exact test counts, artifact names, byte sizes,
digests, skipped Windows VM checks, and elapsed times are recorded in the
final verification report.

## Publication gate

Only after the complete local verification, tracked-content inspection,
history scan, artifact scan, clean Git status, and coherent local commits:

1. recheck the authenticated GitHub account and exact repository-name
   availability;
2. create the public repository with GitHub CLI without `--push`;
3. inspect the generated `origin`;
4. push `main` once with `git push -u origin main`;
5. verify visibility, default branch, remote commit, and clean local status;
6. do not create a release or `v1.0.0` tag.
