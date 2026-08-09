# AI Tool Control Center — Open-Source Windows Platform Design Specification

**Date:** 2026-07-21  
**Status:** Approved with mandatory corrections from the 2026-07-23 technical review
**License:** Apache License 2.0  
**Initial platform:** Windows only  
**Distribution:** NSIS installer and portable ZIP  
**Privacy model:** Local-only, zero telemetry
**Supersedes:** The public-release architecture in the 2026-07-20 local web-app specification. The existing v1 codebase remains a migration source, not the target public architecture.

## 1. Product definition

AI Tool Control Center is a privacy-first Windows desktop application for discovering, reviewing, documenting, monitoring, and safely operating local AI tools and adjacent developer infrastructure.

The public repository contains only the reusable platform, generic demonstration data, adapter schemas, documentation, tests, and packaging logic. It must not contain a maintainer's personal inventory, usernames, local paths, databases, logs, backups, tokens, or private configuration.

The product is intended to answer four questions:

1. What tools and services are present on this Windows computer?
2. What does each item do, and how is it related to the rest of the stack?
3. What changed or requires attention?
4. Which safe operational action can the user take next?

## 2. Locked product decisions

The following decisions are binding for the first public release:

- Windows-only initial release.
- Public project name: **AI Tool Control Center**.
- Apache License 2.0.
- NSIS installer plus portable ZIP.
- Tauri desktop shell.
- React and TypeScript user interface.
- Rust security bridge and native Windows discovery.
- Bundled Python engine for mature product-specific scanners.
- Progressive migration of stable or performance-sensitive scanners from Python to Rust.
- Universal Windows tool discovery rather than Claude/Codex-only scanning.
- Guided quick scan by default and optional deep full-PC scan.
- Every discovery enters a mandatory review queue before import.
- Version 1 supports safe operational controls, not package-management actions.
- Harmless actions may run immediately; execution actions require command preview and confirmation.
- SQLite runtime state plus YAML/JSON user-maintained catalogue data.
- Zero telemetry and no automatic background network access.
- Updates are checked only after a user explicitly requests an update check.
- Initial releases may be unsigned but must be signing-ready and include checksums.
- Built-in scanners plus declarative community adapter packs.
- Community adapter packs cannot execute arbitrary code.

### 2.1 Supported platform for version 1

Version 1 release artifacts target Windows 11 23H2 or newer on x86-64. ARM64
and x86 builds are deferred until the Tauri application, bundled CPython
runtime, native dependencies, installer, portable package, and acceptance
suite are all available for those architectures.

## 3. Primary user experience

### 3.1 Installation artifacts

Each release provides:

```text
AI-Tool-Control-Center-Setup.exe
AI-Tool-Control-Center-Portable.zip
SHA256SUMS.txt
SBOM files
Release notes
```

The installer must:

- install per-user by default;
- avoid requiring administrator privileges for normal installation;
- create a Start Menu shortcut;
- offer an optional Desktop shortcut;
- register a clean Windows uninstall entry;
- bundle every required runtime;
- require no separate Python, Node.js, npm, Rust, or PowerShell package setup;
- preserve user data by default when the program is uninstalled;
- explain potential Windows SmartScreen warnings while releases remain unsigned.

The NSIS build uses per-user installation and the offline WebView2 installer.
Normal uninstall removes application binaries and shortcuts while preserving
the separate user-data directory. Data deletion remains an explicit in-app or
uninstaller choice with a clear backup warning.

Portable mode must run from an extracted folder without installation and store its state under a local `Data` directory.

The portable ZIP is a custom release artifact, not a native Tauri bundle
target. It contains the unbundled application, resources, bundled CPython
engine, and a fixed-version WebView2 runtime so first launch requires no
network access. Before WebView2 starts, portable mode redirects application
state, WebView2 user data, logs, caches, Python temporary files, and crash
artifacts into `Data`. Startup fails safely with a clear message when the
package is read-only or on a network share. Cloud-sync detection is
best-effort and produces a warning, not a security guarantee. Windows may
still retain operating-system-managed traces such as Defender, Prefetch, or
recent-item records.

### 3.2 First-run wizard

The first launch contains five concise stages:

1. Welcome
2. Privacy and storage
3. Scan scope
4. Review discoveries
5. Open dashboard

The wizard must be keyboard accessible, resumable after interruption, and dismissible only after the user understands where local data is stored.

### 3.3 Scan choices

#### Quick guided scan

Checks safe and common locations, including:

- Claude and Codex configuration and skill locations;
- common AI-tool configuration folders;
- Windows installed applications;
- PATH executables;
- Windows services;
- scheduled tasks relevant to detected tools;
- Docker Desktop and containers;
- common launcher folders;
- user-selected folders.

#### Deep PC scan

Scans additional selected drives and folders with:

- visible progress;
- current location display;
- pause;
- resume;
- cancellation;
- exclusions;
- independent scanner timeouts;
- protection against symbolic-link and junction cycles.

The deep scan excludes high-noise and high-cost directories by default, including `.git`, `node_modules`, virtual environments, package caches, browser caches, and system restore data.

Filesystem scanners do not follow reparse points by default and do not
hydrate cloud placeholders marked offline or recall-on-data-access. Following
reparse points is an explicit per-scan option; when enabled, traversal tracks
volume and file identity to prevent cycles. UNC and network roots require
explicit consent for each scan. Pause and resume are cooperative at safe
scanner checkpoints. Scanners that cannot pause must remain cancellable and
bounded by a hard timeout.

## 4. Mandatory review queue

No discovery enters the primary inventory automatically.

Each discovery shows:

- detected name;
- suggested type;
- source and path;
- host or platform;
- scanner and adapter responsible;
- explainable confidence;
- reason it was detected;
- possible duplicates;
- proposed dependencies and relationships;
- fields inferred by the scanner;
- fields still unknown.

Available decisions:

- Import
- Edit and import
- Merge with an existing capability
- Ignore once
- Always ignore
- Keep as unknown
- Open detected location

Bulk review is supported, but ambiguous merges, executable actions, or low-confidence classifications must never be silently accepted.

Ignored records retain a fingerprint so later scans can distinguish an intentionally ignored item from a genuinely new discovery.

## 5. Domain model

### 5.1 Canonical capability

Represents the underlying product or function, such as Playwright, Figma, Ollama, or OpenDesign.

### 5.2 Installation instance

Represents one concrete installation or registration route, such as:

- Playwright as a Claude MCP;
- Playwright as a Codex MCP;
- Playwright CLI on PATH;
- Playwright package inside a project.

Multiple instances may belong to one canonical capability without losing instance-specific paths, versions, commands, hosts, or health evidence.

### 5.3 Recognized types

- MCP
- Plugin
- Skill
- Launcher
- Local service
- Windows service
- CLI
- Desktop application
- Docker container
- Runtime
- Configuration
- Adapter
- Collection
- Unknown

### 5.4 Relationships

Typed relationships include:

- provides
- starts
- stops
- requires
- depends on
- registered in
- installed by
- bundled with
- configured by
- authenticates through
- duplicates
- overlaps with
- exposes port
- runs process

### 5.5 Evidence

Every machine-managed fact records:

- source scanner;
- interpreting adapter;
- observed path, registry key, command, process, service, or port;
- observation time;
- confidence;
- sanitized raw evidence where useful;
- whether the field is scanner-managed or user-managed.

User-authored notes, descriptions, aliases, tags, documentation links, and manual relationships must not be overwritten by later scans.

## 6. Status model

The interface must not collapse all state into one colour.

### 6.1 Installation state

- Detected
- Imported
- Installed
- Missing
- Setup incomplete
- Unknown

### 6.2 Registration state

- Registered
- Not registered
- Not applicable
- Unknown

### 6.3 Enablement state

- Enabled
- Disabled
- Not applicable
- Unknown

### 6.4 Runtime state

- Running
- Stopped
- Starting
- Stopping
- Unreachable
- Not applicable
- Unknown

### 6.5 Connection state

- Connected
- Disconnected
- Unreachable
- Not applicable
- Unknown

### 6.6 Authentication state

- Authentication required
- Authentication detected
- Authentication invalid
- Not applicable
- Unknown

### 6.7 Health state

- Green: all configured required checks passed.
- Yellow: usable but degraded, incomplete, duplicated, stale, or requiring review.
- Red: one or more required checks failed.
- Grey: not checked or insufficient evidence.
- Blue: setup or user action is required.

A green result means the configured checks passed. It must never be described as proof that a tool, skill, prompt, or integration is universally optimal.

## 7. Desktop architecture

```text
Tauri desktop shell
├── React + TypeScript interface
├── Rust command and security bridge
├── Rust native Windows discovery engine
├── Bundled Python product-specific scanner engine
├── SQLite runtime database
└── YAML/JSON user catalogue and adapter packs
```

### 7.1 Tauri responsibilities

- desktop lifecycle;
- window management;
- native menus and notifications;
- installer integration;
- update-check initiation after explicit user action;
- native file and folder dialogs;
- safe exposure of Rust commands to the frontend.

### 7.2 React responsibilities

- onboarding;
- dashboard navigation;
- inventory and review workflows;
- detail workspaces;
- health presentation;
- dependency graph;
- action confirmations;
- backups and settings;
- contextual help and glossary.

React cannot execute arbitrary local commands directly.

The frontend receives no Tauri shell or unrestricted filesystem capability.
Only the local main webview may call the minimum registered Rust commands;
remote origins receive no capabilities. A strict content-security policy
blocks remote scripts, remote frames, and unapproved connections. Sidecars,
files, processes, services, updates, and actions are reachable only through
Rust commands that repeat application-level authorization.

### 7.3 Rust responsibilities

Initial Rust modules cover:

- Windows path normalization;
- filesystem traversal;
- Registry inspection;
- installed application discovery;
- process discovery;
- Windows service discovery;
- open-port discovery;
- scheduled-task discovery;
- launcher metadata extraction;
- `.lnk` resolution where supported;
- task cancellation and timeout enforcement;
- command validation and execution;
- privilege escalation boundary;
- redaction and structured audit output.

### 7.4 Python responsibilities

Python is retained initially for adapters whose parsing logic already exists or changes frequently:

- Claude Code MCP, plugin, and skill parsing;
- Codex MCP, plugin, and skill parsing;
- Docker and CLI-output parsing;
- third-party product adapters;
- complex manifest and configuration interpretation.

Python runs as a bundled private engine. It must not depend on a user-installed Python interpreter.

The release pins an x86-64 CPython 3.14 embeddable runtime and its SHA-256
digest. Its licence and required third-party notices ship in every binary
artifact. The controlled `._pth` isolation remains enabled, runtime `pip` is
not included or invoked, and any third-party wheels are vendored from a
hash-locked build input. Rust launches the absolute bundled interpreter in
isolated mode with Python-specific environment variables removed and an
application-controlled working directory.

### 7.5 Migration strategy

Scanner interfaces are language-neutral. Rust and Python scanners emit the same versioned discovery contract. Stable scanners may be migrated to Rust without changing frontend or database contracts.

## 8. Scanner architecture

### 8.1 Layer 1: Native Windows discovery

Collects observable facts without over-classifying them:

- applications;
- executables;
- files;
- services;
- processes;
- ports;
- registry records;
- scheduled tasks;
- launchers;
- shortcuts;
- containers;
- common configuration roots.

### 8.2 Layer 2: Built-in deep adapters

Version 1 includes deep adapters for at least:

- Claude Code;
- Codex;
- common MCP configuration formats;
- Docker;
- Ollama;
- OpenWebUI;
- n8n;
- Supabase;
- Postiz;
- Stirling PDF;
- Playwright;
- Firecrawl.

Adapters must distinguish existence, installation, registration, enablement, authentication, running state, connectivity, and health.

### 8.3 Layer 3: Declarative community adapter packs

Adapter packs may define:

- adapter identity and version;
- supported application versions;
- product metadata;
- common paths;
- executable names;
- registry locations;
- known launcher names;
- typed version probes implemented by the core;
- typed process, service, port, file, registry, and loopback checks;
- relationships;
- documentation links;
- typed start, stop, restart, open, and log-viewing operations implemented by
  the core;
- plain-language descriptions and tooltips.

Adapter packs may not:

- execute arbitrary Python, Rust, JavaScript, PowerShell, CMD, or native code;
- read unrestricted files;
- access secret values;
- transmit data;
- run an action without the platform's permission and confirmation boundary.

Community packs cannot provide executable paths, shell text, raw argument
templates, scripts, or administrator commands. Raw command definitions are
limited to reviewed built-in source or explicit user-authored commands and
remain subject to the same Rust validation and confirmation boundary.

### 8.4 Scanner isolation and anti-hang rules

- Long-running scanners execute outside the UI thread.
- Each scanner has an independent timeout.
- One scanner failure cannot block another scanner.
- Results stream incrementally into the review queue.
- Navigation remains usable during scans.
- Users can pause or cancel deep scans.
- Repeated junction, symbolic-link, and filesystem cycles are prevented.
- Output and logs are bounded.
- Scanner processes are terminated when cancellation or hard timeout succeeds.
- If termination fails, the scanner is quarantined and the application remains usable.

Native Rust scanners are isolated by task/result boundaries and convert
panics, access-denied results, and malformed observations into per-scanner
failures. Python scanners use one child process tree per invocation, a
versioned newline-delimited JSON protocol, bounded messages, bounded stderr,
and a Windows Job Object configured to terminate descendants on close.

### 8.5 Explainable confidence

- High: exact executable or configuration plus matching adapter evidence.
- Medium: known path, registration, or launcher with incomplete corroboration.
- Low: generic heuristic or unclear classification.

Confidence changes the suggestion only. It never bypasses review.

## 9. Main information architecture

### 9.1 Navigation

- Overview
- Inventory
- Review Queue
- Health
- Dependencies
- Activity
- Adapter Packs
- Backups
- Settings

Persistent top bar:

- global search;
- scan action;
- pending-review count;
- health alerts;
- current scan progress;
- theme control.

### 9.2 Overview

Shows:

- reviewed tools;
- pending discoveries;
- healthy, warning, failed, and unknown items;
- running and stopped services;
- recent discoveries;
- recent health failures;
- duplicate or conflicting registrations;
- recent actions;
- quick scan, add-folder, import, and manual-add actions.

Counts must state whether they refer to canonical capabilities, installation instances, or pending discoveries.

### 9.3 Inventory

Supports table and card views with filters for:

- type;
- platform or host;
- health;
- runtime state;
- review status;
- source scanner;
- adapter;
- tag;
- drive or folder.

Search matches:

- names and aliases;
- descriptions;
- paths;
- commands;
- ports;
- tags;
- providers;
- related tools.

Large result sets use virtualization.

### 9.4 Item workspace

Tabs:

- Overview
- Installations
- Usage
- Configuration
- Health
- Dependencies
- Actions
- Files and logs
- History
- Notes

The workspace may show:

- plain-language purpose;
- installed versions;
- detection evidence;
- related hosts;
- executable and arguments;
- configuration paths;
- required runtimes;
- ports and processes;
- authentication presence without secrets;
- last observation and health check;
- troubleshooting guidance;
- user notes, tags, and aliases.

### 9.5 Hover and focus previews

Hovering or keyboard-focusing an item shows a concise, non-blocking summary containing name, purpose, type, host, health, last check, and primary dependency.

Tooltips must be available to keyboard and assistive-technology users and must not trap focus.

## 10. Safe operational controls

Version 1 supports safe controls only.

### 10.1 Immediate actions

- Open application
- Open folder
- Copy command
- View logs
- View documentation
- Rescan

### 10.2 Confirmed actions

- Start
- Stop
- Restart
- Run launcher
- Run approved PowerShell or batch command
- Run a narrowly scoped administrator action

The confirmation screen shows:

- exact executable and structured arguments;
- working directory;
- privilege requirement;
- expected process or service;
- timeout;
- likely effect;
- available stop or rollback action.

### 10.3 Out of scope for version 1

- automatic installation;
- automatic update of third-party tools;
- repair;
- disable or enable changes;
- uninstall;
- arbitrary package-management commands;
- executable third-party plugin code.

## 11. Command security boundary

All command execution flows through the Rust bridge:

```text
React action request
→ inventory and permission validation
→ command preview generation
→ required user confirmation
→ restricted execution
→ output redaction
→ local audit record
```

The bridge rejects:

- commands not attached to an approved inventory item;
- unsafe shell interpolation;
- shell metacharacter injection from editable fields;
- hidden background execution;
- unbounded administrator commands;
- executable logic supplied by community adapter packs;
- actions that transmit inventory data without explicit user action.

Arguments are passed as structured arrays rather than concatenated shell strings whenever possible.

Normal application operation is unelevated. Administrator actions use a short-lived helper for one approved command and then exit.

Administrator actions are typed operations, never generic executable or
shell requests. They are disabled in unsigned builds. A signing-enabled build
may expose them only through an Authenticode-signed first-party helper that
revalidates the structured request after elevation, authenticates local IPC,
uses safe DLL search rules, and exits after the single operation.

## 12. Local data architecture

### 12.1 Installed mode

```text
%LOCALAPPDATA%\Programs\AI Tool Control Center\
%LOCALAPPDATA%\AI Tool Control Center\
├── database\control-center.db
├── catalog\tools.yaml
├── catalog\aliases.yaml
├── catalog\ignored-items.yaml
├── catalog\adapter-settings.yaml
├── adapters\community\
├── logs\
├── backups\
└── cache\
```

### 12.2 Portable mode

```text
AI-Tool-Control-Center-Portable\Data\
```

Portable mode warns when the folder is located in a cloud-synced directory.

### 12.3 SQLite responsibilities

- discoveries;
- reviewed installation instances;
- health results;
- scan history;
- action history;
- runtime observations;
- relationships;
- versions;
- adapter execution results.

### 12.4 YAML and JSON responsibilities

- custom descriptions;
- aliases;
- tags;
- notes;
- approved custom safe commands;
- ignored detections and paths;
- selected scan folders;
- adapter preferences;
- manual relationships;
- portable catalogue exports.

SQLite is authoritative for machine observations, review decisions,
fingerprints, runtime state, history, and normalized relationships. YAML/JSON
is authoritative only for user-authored aliases, descriptions, tags, notes,
manual relationships, selected folders, ignore rules, and adapter
preferences. Projections carry a schema version and never become a competing
writer.

Database migrations are versioned and transactional. Before migration, the
application creates a consistent backup using SQLite's online backup API or
`VACUUM INTO`; it never copies a live WAL database. Migrations run against a
temporary candidate and replace the active database only after integrity and
relationship validation. A failed migration leaves the prior database
untouched. Catalogue files use write-to-temporary, flush, and atomic replace.
One process owns a data directory at a time.

## 13. Privacy and credential handling

The application may show credential presence:

```text
GITHUB_TOKEN: configured
FIGMA_TOKEN: missing
Claude OAuth session: detected
```

It must not show or export:

- environment-variable values;
- API keys;
- OAuth tokens;
- passwords;
- cookies;
- Authorization headers;
- private-key contents;
- credential-bearing connection strings.

Redaction covers common token formats, password-like keys, authorization headers, cookies, private keys, and secret environment variables.

The user may explicitly open an original configuration file after a warning that it may contain secrets.

## 14. Zero telemetry and networking

AI Tool Control Center-owned code initiates no telemetry or automatic
background network requests. Separately serviced operating-system components,
including Evergreen WebView2 maintenance used by installed mode, are governed
by Microsoft policy and are disclosed independently. Tests distinguish
application-originated traffic from operating-system runtime traffic.

Allowed network actions require an explicit user gesture:

- open documentation;
- open an official website;
- check for updates;
- download an approved update.

Non-loopback health checks also require a user gesture and preview the exact
destination. Loopback probes are bounded, declared by reviewed core code or a
validated typed adapter operation, and never transmit inventory content.

The update check shows current version, available version, release notes,
checksum, and download size before downloading. Check, download, and install
are separate approvals. Tauri updater signatures are mandatory and distinct
from SHA-256 integrity files and Authenticode signatures. Installed builds use
interactive NSIS updater artifacts; portable builds direct the user to a
manual ZIP download and replacement workflow. No update is installed
silently.

A repository test must fail when background networking is introduced without an explicit allow-list entry and user-triggered path.

## 15. Health checks

Health checks are safe, bounded, and type-specific.

### 15.1 MCP

- registration exists;
- executable or endpoint exists;
- configuration parses;
- authentication presence is detectable;
- safe handshake or tool listing succeeds where supported;
- latency remains within adapter-defined limits.

### 15.2 Plugin and skill

- installation record or manifest exists;
- enabled state is known;
- declared files exist;
- required references resolve;
- dependencies are present;
- duplicate installations are identified;
- structural metadata is valid.

### 15.3 Launcher and service

- launcher exists;
- target command and working directory exist;
- expected process, service, port, or container is observable;
- start and stop relationships are known where possible;
- recent failures are summarized and redacted.

Health checks must not trigger paid calls, create external content, mutate accounts, or perform destructive operations.

## 16. Backups, diagnostics, and deletion

### 16.1 Full private backup

May contain database, catalogue, notes, aliases, relationships, ignored items, adapter settings, and history. It is intended only for the user and may contain local paths.

### 16.2 Sanitized diagnostic bundle

Contains application version, Windows version, adapter versions, redacted errors, scanner timing, schema version, and selected health outcomes.

It excludes personal inventory names unless the user approves them, usernames in paths, secrets, private notes, and sensitive command output.

The application presents an exact file preview before creating the bundle.

### 16.3 Restore

Restore must:

1. validate format and checksum;
2. show backup date and source version;
3. back up current state;
4. import into temporary storage;
5. validate schema and relationships;
6. replace current state only after validation;
7. preserve a rollback path.

### 16.4 Retention defaults

- application and scanner logs: 30 days;
- health history: 90 days;
- installer logs: 14 days;
- action history: retained until the user deletes it.

Logs are redacted, size-limited, and configurable.

### 16.5 User deletion controls

- clear scan history;
- clear action history;
- clear logs;
- reset review queue;
- delete imported inventory;
- reset application;
- open data folder.

Uninstall preserves data by default and asks before removing local inventory and backups.

## 17. Adapter-pack format and governance

The repository provides:

- `adapter.schema.json`;
- `adapter.example.yaml`;
- adapter validation CLI;
- fixtures and tests;
- authoring documentation.

Validation rejects:

- unknown fields;
- invalid path or environment-variable syntax;
- duplicate adapter IDs;
- unsafe command templates;
- unlabelled network actions;
- shell injection patterns;
- unsupported action types;
- missing documentation;
- incompatible application-version declarations.

Community adapters require schema validation, detection evidence, at least one fixture, safe health checks, and no executable custom logic.

## 18. Public repository

Recommended structure:

```text
ai-tool-control-center/
├── apps/
│   ├── desktop/
│   └── frontend/
├── engine/
│   ├── rust/
│   └── python/
├── adapters/
│   ├── built-in/
│   ├── community/
│   └── schema/
├── packaging/
│   ├── nsis/
│   └── portable/
├── docs/
│   ├── getting-started/
│   ├── user-guide/
│   ├── adapter-authoring/
│   ├── architecture/
│   ├── troubleshooting/
│   └── security/
├── examples/
│   ├── demo-catalog/
│   └── adapter-packs/
├── tests/
└── .github/
    ├── workflows/
    ├── ISSUE_TEMPLATE/
    └── PULL_REQUEST_TEMPLATE.md
```

Required governance files:

- `LICENSE` with Apache License 2.0;
- `README.md`;
- `CONTRIBUTING.md`;
- `CODE_OF_CONDUCT.md`;
- `SECURITY.md`;
- `SUPPORT.md`;
- `ROADMAP.md`;
- pull-request template;
- issue templates.

The public repository includes generic demo data only.

## 19. Documentation experience

The README leads with:

1. Download
2. Scan
3. Review and manage

It includes screenshots, installer and portable links, supported Windows versions, privacy statement, five-minute quick start, terminology, and troubleshooting.

Documentation must explain MCP, plugin, skill, launcher, runtime, service, CLI, adapter, and canonical capability in plain language.

In-app help includes:

- first-run walkthrough;
- optional guided tour;
- contextual page explanations;
- empty-state guidance;
- searchable glossary;
- copyable diagnostic steps;
- restartable walkthrough from Settings.

## 20. Testing strategy

### 20.1 Rust

- path normalization;
- registry parsing;
- process, service, port, and scheduled-task discovery;
- launcher resolution;
- command validation;
- privilege boundary;
- cancellation and timeout;
- exclusions;
- filesystem-cycle prevention;
- redaction.

### 20.2 Python

- Claude parsing;
- Codex parsing;
- MCP, plugin, and skill discovery;
- Docker and third-party CLI parsing;
- adapter interpretation;
- malformed output;
- missing executables;
- timeout isolation;
- redaction.

### 20.3 Frontend

- navigation while scans are active;
- mandatory review workflow;
- duplicate merge flow;
- inventory filtering and search;
- item workspace loading;
- action preview and confirmation;
- keyboard navigation and tooltips;
- large-list virtualization;
- error and offline recovery;
- first-run onboarding.

### 20.4 Windows end-to-end

- NSIS install;
- portable launch;
- first-run wizard;
- quick scan;
- review and import;
- deep-scan cancellation;
- start/stop confirmation;
- backup and restore;
- uninstall;
- user-data preservation;
- manual update check.

A deliberately slow scanner is mandatory to prove that the application remains interactive while scanning.

## 21. Continuous integration and release pipeline

Every pull request runs:

- Rust format, lint, and tests;
- Python lint and tests;
- TypeScript lint and tests;
- frontend production build;
- Tauri compile checks;
- adapter-schema validation;
- secret scan;
- dependency audit;
- background-network allow-list test;
- Windows integration tests.

Pull-request CI pins the runner image, Rust/Node/Python toolchains, dependency
locks, and third-party Actions by full commit SHA. GitHub-hosted Windows
runners provide build and smoke-test coverage only. Installer, portable,
standard-user, UAC, WebView2, SmartScreen, uninstall, and user-data-preservation
acceptance runs on a controlled clean Windows 11 desktop VM.

Tagged releases:

- build NSIS installer;
- build portable ZIP;
- generate SHA-256 checksums;
- generate software bill of materials;
- attach release notes;
- publish GitHub Release artifacts.

The packaging pipeline has an explicit signing stage. When signing is
enabled, it signs and verifies first-party executables, DLLs, and helpers
before packaging, then signs and verifies the NSIS installer with SHA-256 and
an RFC 3161 timestamp. Checksums, updater signatures, per-artifact SPDX or
CycloneDX SBOMs, licence bundles, build provenance, and GitHub attestations
are produced from the final bytes. Unsigned builds omit administrator actions.

Release repeatability means a clean build from locked inputs with a
`BUILDINFO` record containing the source commit, toolchains, runner image, and
lockfile hashes. Bit-for-bit reproducibility is claimed only after two
independent unsigned builds compare equal; timestamped signed artifacts are
not described as bit-for-bit reproducible.

## 22. Versioning and compatibility

The application follows semantic versioning:

- patch: bug fixes;
- minor: new adapters and backward-compatible features;
- major: breaking storage, adapter-schema, security, or API changes.

Adapter packs have independent versions and declare the supported application version range.

Database and adapter-schema versions are separate from the desktop application version.

## 23. Version 1.0 scope

Version 1.0 includes:

- Windows NSIS installer and portable ZIP;
- Tauri shell;
- onboarding;
- quick and deep scans;
- mandatory review queue;
- universal Windows discovery;
- Claude and Codex deep adapters;
- Docker and common local-service detection;
- inventory and detailed workspaces;
- health checks;
- dependency relationships;
- safe start, stop, restart, open, copy, log, and rescan actions;
- local backups and sanitized diagnostics;
- community declarative adapters;
- zero telemetry;
- user-triggered update checks;
- Apache 2.0 public repository.

Version 1.0 excludes:

- macOS and Linux;
- cloud sync;
- accounts;
- remote access;
- background updates;
- automatic tool installation or uninstall;
- arbitrary executable extensions;
- automatic import without review;
- automatic transmission of diagnostics or inventory.

## 24. Performance and reliability acceptance criteria

- Normal navigation remains responsive during all scans.
- No scanner runs on the frontend UI thread.
- A failed or hung scanner does not prevent other results from appearing.
- Every long operation is cancellable or has a hard timeout.
- Quick scan exposes meaningful progress and completes without searching excluded high-noise directories.
- Deep scans stream results and expose current scan location.
- Large inventories remain usable through virtualization and indexed search.
- Startup requires no external development runtime.
- The app can launch without internet access.
- Closing and reopening the desktop application preserves reviewed inventory and pending discoveries.
- Failed database migration or restore cannot overwrite the last working state.

## 25. Security and privacy acceptance criteria

- No telemetry or automatic network calls occur.
- Secret-shaped values are redacted from UI, logs, exports, diagnostics, and action history.
- Community adapter packs cannot run arbitrary code.
- Commands are validated and tied to approved inventory items.
- Confirmed actions show exact commands and effects before execution.
- Administrator execution is narrow and short-lived.
- Public source and release artifacts contain no private inventory, usernames, personal paths, logs, databases, backups, or credentials.
- Source history, candidate tracked and untracked files, dependency
  configuration, unpacked installers, portable archives, PDB/source-map
  metadata, and final artifacts pass redacted secret, private-source, and
  personal-path inspection before publication.

## 26. Usability acceptance criteria

A first-time user can:

1. install or extract the application without installing development tools;
2. understand the privacy model;
3. complete a quick scan;
4. review and import discoveries;
5. search and filter the inventory;
6. open a detailed item workspace;
7. understand status and evidence;
8. run a confirmed safe action;
9. create a backup;
10. find troubleshooting help.

The interface must remain usable with keyboard-only navigation and must provide accessible names, focus states, and non-hover alternatives for all tooltips and actions.

## 27. Implementation sequence

The approved design should be implemented as separate testable workstreams:

1. Public-repository sanitization and governance foundation.
2. Shared discovery contract and storage migration design.
3. Tauri shell and Rust permission bridge.
4. Native Windows discovery modules.
5. Bundled Python engine integration.
6. Mandatory review queue and duplicate resolution.
7. Inventory, details, health, and dependency workspaces.
8. Safe command execution and action audit.
9. Adapter schema, validator, and community examples.
10. Backup, restore, diagnostics, and deletion.
11. NSIS and portable packaging.
12. GitHub Actions, reproducibility, checksums, and release documentation.

Each workstream requires a dedicated implementation plan and an independently verifiable deliverable.

## 28. 2026-07-23 technical-review decision log

The written-spec review concluded **Approved with corrections**. No locked
product decision was rejected. The following substantive corrections preserve
the approved intent while removing contradictions or incomplete release
requirements:

1. Separated installation, registration, enablement, runtime, connection, and
   authentication states because the prior installation list collapsed
   independent dimensions.
2. Restricted community adapters to typed core primitives because raw command
   templates would contradict the locked ban on executable third-party logic.
3. Removed shell and unrestricted filesystem authority from React and required
   local-only capabilities plus a strict CSP because Tauri capabilities do not
   replace application authorization.
4. Defined the portable ZIP as a custom artifact and selected fixed-version
   WebView2 for it; selected the offline WebView2 installer for NSIS. Tauri has
   no portable target and its default installer mode downloads a bootstrapper.
5. Defined CPython embeddable isolation, hash pinning, vendoring, IPC bounds,
   and Windows Job Object lifetime because merely saying “bundled Python” did
   not prevent system-Python fallback or orphan scanners.
6. Made filesystem pause cooperative, disabled reparse traversal by default,
   added file-identity cycle tracking, and gated UNC/cloud hydration because a
   deep scan must not trigger unbounded or remote I/O.
7. Assigned canonical ownership between SQLite and YAML/JSON and required
   consistent SQLite backups because copying a live WAL database is not a safe
   migration or restore primitive.
8. Limited elevated controls to typed, signed-helper operations and disabled
   them in unsigned builds because a generic unsigned helper in a user-writable
   install directory is not a defensible privilege boundary.
9. Scoped zero telemetry to application-controlled behavior and separately
   disclosed WebView2/Windows servicing because the application cannot enforce
   machine-wide network silence.
10. Separated updater signatures, Authenticode, checksums, SBOMs, provenance,
    and attestations because each supplies a different security property.
11. Split hosted-runner smoke tests from clean Windows desktop acceptance
    because GitHub-hosted images include development tools and do not model
    SmartScreen or standard-user UAC behavior.
12. Defined Windows 11 x86-64 as the initial support target because the desktop
    binary, sidecars, Python runtime, WebView2 package, and native dependencies
    are architecture-specific.

Primary evidence:

- Tauri capabilities and CSP:
  <https://v2.tauri.app/security/capabilities/> and
  <https://v2.tauri.app/security/csp/>
- Tauri sidecars and Windows packaging:
  <https://v2.tauri.app/develop/sidecar/> and
  <https://v2.tauri.app/distribute/windows-installer/>
- Tauri updater signatures:
  <https://v2.tauri.app/plugin/updater/>
- Microsoft WebView2 distribution and privacy:
  <https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution>
  and
  <https://learn.microsoft.com/microsoft-edge/webview2/concepts/data-privacy>
- CPython embeddable distribution:
  <https://docs.python.org/3/using/windows.html#the-embeddable-package>
- Windows Job Objects and reparse points:
  <https://learn.microsoft.com/windows/win32/procthread/job-objects> and
  <https://learn.microsoft.com/windows/win32/fileio/reparse-point-operations>
- SQLite backup and WAL behavior:
  <https://sqlite.org/backup.html> and <https://sqlite.org/wal.html>
- GitHub-hosted runners and Actions hardening:
  <https://docs.github.com/actions/reference/runners/github-hosted-runners>
  and
  <https://docs.github.com/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions>
