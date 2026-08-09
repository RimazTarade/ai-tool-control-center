# AI Tool Control Center open-source Windows specification vetting

**Review date:** 2026-07-23  
**Specification:** `docs/superpowers/specs/2026-07-21-ai-tool-control-center-open-source-windows-design.md`  
**Verdict:** Approved with corrections

## Executive conclusion

The approved Tauri, React/TypeScript, Rust, bundled-Python, SQLite, and
declarative-adapter architecture is feasible for a Windows 11 x86-64
application. No locked product decision requires a redesign.

The original text was not implementation-ready in four areas:

1. raw community command templates contradicted the ban on executable
   third-party logic;
2. portable and offline WebView2 distribution were not defined;
3. Python process isolation and unsigned elevation were not safe enough;
4. storage ownership, zero-network wording, and release evidence were
   underspecified.

The reviewed specification corrects these points while preserving the
approved product intent.

## Repository state

- New Git repository on the `main` branch with no pre-existing history or
  remote.
- Initial input consisted only of the authoritative specification and ignored
  local tool metadata.
- No application source, dependency manifests, lockfiles, generated artifacts,
  runtime data, or unrelated user files existed before implementation.
- Local build prerequisites include a stable x86-64 MSVC Rust toolchain,
  Node.js 24, pnpm 10, CPython, GitHub CLI, Visual Studio Build Tools, Windows
  SDK, and WebView2 Evergreen Runtime.
- The target GitHub repository name was not present under the authenticated
  account when checked. No remote repository was created during vetting.

## Strengths

- The mandatory review queue prevents scanner guesses from becoming accepted
  inventory.
- Capability and installation-instance separation preserves multiple
  registrations and hosts.
- Evidence, confidence, source, and observation time prevent historical or
  inferred data from masquerading as current health.
- Rust is correctly placed at the command and privilege boundary.
- A language-neutral scanner contract permits gradual Python-to-Rust
  migration.
- Local-only storage, user-triggered networking, and generic public demo data
  establish a sound privacy baseline.
- Cancellable scanner work and navigation-during-scan regression coverage
  directly address prior prototype failures.

## Contradictions

### Community adapters and executable templates

The specification allowed community-defined version commands and start/stop
templates while also prohibiting executable community logic. A schema cannot
make arbitrary shell text safe. The corrected design permits only typed core
operations such as service status, process presence, loopback probe, open
folder, and service start/stop. Raw commands remain reviewed built-in source or
explicit user-authored data.

### Separate status dimensions

The installation-state list included registered, enabled, and disabled values
despite the product decision that installation, registration, and enablement
remain independent. The corrected model gives each dimension its own enum.

### Offline operation and WebView2

Tauri's default NSIS configuration downloads a WebView2 bootstrapper when the
runtime is absent. That conflicts with offline installation and the requirement
to bundle required runtimes. The corrected design uses an offline WebView2
installer for NSIS and a fixed-version runtime in the custom portable ZIP.

### Unsigned releases and elevation

An unsigned generic helper installed under a user-writable directory cannot
provide a strong administrator boundary. The corrected design disables
administrator operations in unsigned builds and later enables only typed
operations through a signed, revalidating helper.

## Missing requirements

The review added:

- Windows 11 23H2+ x86-64 as the initial supported target;
- custom portable staging and writable-data checks;
- strict Tauri capabilities, local-only origins, and CSP;
- fixed CPython embeddable architecture, hash, isolation, and licence handling;
- Windows Job Object ownership of Python process trees;
- default refusal to follow reparse points or hydrate cloud placeholders;
- explicit UNC consent and cooperative pause semantics;
- canonical ownership between SQLite and YAML/JSON;
- consistent SQLite backup and atomic catalogue replacement;
- separate update check, download, and install approvals;
- distinction between updater signatures, Authenticode, checksums, SBOMs, and
  attestations;
- hosted-runner smoke coverage versus clean desktop acceptance;
- traceable repeatable builds without an unsupported reproducibility claim.

## Security concerns

### Desktop IPC

React must not receive shell or unrestricted filesystem permissions. Tauri
capabilities and CSP narrow the exposed surface, but Rust commands must still
authorize inventory ownership, allowed operation, structured arguments,
working directory, timeout, and expected effect.

### Command execution

No action may concatenate a shell command. Built-in and user-authored actions
use executable-plus-argument arrays. Shell interpreters require stricter
policy, explicit preview, no encoded or hidden command mode, bounded output,
and confirmation. Community packs cannot supply them.

### Elevation

Unsigned builds expose no administrator actions. A future signed helper
accepts a typed operation, authenticates same-user local IPC, revalidates after
UAC elevation, uses safe DLL search behavior, and exits after one operation.

### Scanner containment

Each Python scanner invocation receives bounded JSON Lines input and output,
bounded stderr, a hard timeout, cancellation, a sanitized environment, and a
kill-on-close Job Object. Rust scanners return per-scanner errors and do not
turn access denied or malformed evidence into a process-wide failure.

### Supply chain

All ecosystems require committed locks and public sources. CI rejects
credential-bearing URLs, local or UNC dependencies, unpinned Git sources,
private registries, and generated artifacts containing private paths or
secret-shaped data.

## Privacy concerns

- “Zero telemetry” now applies to application-owned code. Windows and
  separately serviced WebView2 behavior is disclosed rather than falsely
  claimed controllable.
- Non-loopback checks require an explicit gesture and exact destination
  preview.
- Filesystem scans do not follow reparse points, enter UNC roots, or hydrate
  offline cloud content by default.
- Diagnostics use an exact preview and redact usernames, secrets, authorization
  data, private keys, connection strings, and sensitive command output.
- Private backups may contain local paths and notes and are labelled
  accordingly; they are never telemetry or automatic uploads.

## Packaging concerns

- Tauri provides NSIS/MSI bundles, not a portable ZIP target.
- The portable package therefore needs its own staging, adjacent `Data`
  routing, fixed WebView2 runtime, CPython runtime, archive manifest, checksum,
  and startup test.
- Installed mode uses per-user NSIS plus the offline WebView2 installer.
- Portable startup must reject read-only and network-share data locations.
- Windows can retain OS-managed traces even when application data is portable.
- Tauri updater signatures are mandatory and separate from future
  Authenticode signing.

## Licensing concerns

- Root and binary distributions include Apache-2.0.
- CPython, WebView2, NSIS, Rust crates, npm packages, fonts, icons, and other
  shipped assets require a generated licence inventory and reviewed notices.
- `NOTICE` is created only when project attribution or inherited notices
  require it.
- Automated licence metadata is evidence for review, not a legal conclusion.

## Testability concerns

- GitHub-hosted Windows images contain development runtimes and do not model a
  normal Windows desktop, SmartScreen, standard-user UAC, or clean uninstall.
- CI can prove compilation, unit behavior, browser interaction, packaging
  assembly, and sanitized-PATH smoke behavior.
- Controlled Windows 11 desktop VM acceptance is required before a release can
  claim clean-machine install, portable launch, UAC, uninstall, and
  user-data-preservation coverage.
- Timestamped Authenticode outputs are not bit-for-bit reproducible. The first
  release guarantee is traceable, locked, repeatable input and build evidence.

## Required specification edits

All required edits are recorded in section 28 of the reviewed specification:

1. independent status dimensions;
2. typed community adapter operations;
3. least-privilege Tauri IPC and CSP;
4. custom portable and offline WebView2 packaging;
5. pinned, isolated CPython and bounded process trees;
6. reparse, cloud-placeholder, UNC, pause, and cancellation rules;
7. unambiguous storage ownership and safe migration backup;
8. signed-only typed elevation;
9. precise application-controlled zero telemetry;
10. distinct release-integrity mechanisms;
11. clean desktop acceptance outside hosted CI;
12. explicit Windows and architecture support.

## Required implementation changes

- Put domain, storage, scanner orchestration, redaction, and command validation
  in a reusable Rust core crate.
- Keep the Tauri crate thin and expose only typed commands.
- Use versioned JSON Lines between Rust and a private CPython sidecar.
- Stream scan events and persist review decisions without blocking navigation.
- Reject unreviewed or unknown executable actions at the Rust boundary.
- Validate adapters with a closed schema and typed operation discriminators.
- Build installer and portable artifacts from explicit staging manifests.
- Scan source, history, locks, unpacked artifacts, PDBs, and source maps before
  publication.

## Deferred recommendations

- ARM64 after all native and Python dependencies plus packaging tests exist.
- Authenticode-backed administrator helper after a signing identity and
  protected signing environment exist.
- Rust migration of stable Python scanners based on measured startup,
  throughput, or reliability.
- Bit-for-bit reproducibility only after two independent unsigned builds
  compare equal.
- Additional built-in product adapters after the scanner contract and review
  workflow are stable.

## Evidence

- [Tauri capabilities](https://v2.tauri.app/security/capabilities/)
- [Tauri content security policy](https://v2.tauri.app/security/csp/)
- [Tauri sidecars](https://v2.tauri.app/develop/sidecar/)
- [Tauri Windows installer](https://v2.tauri.app/distribute/windows-installer/)
- [Tauri updater](https://v2.tauri.app/plugin/updater/)
- [Microsoft WebView2 distribution](https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution)
- [Microsoft WebView2 data and privacy](https://learn.microsoft.com/microsoft-edge/webview2/concepts/data-privacy)
- [CPython embeddable package](https://docs.python.org/3/using/windows.html#the-embeddable-package)
- [Windows Job Objects](https://learn.microsoft.com/windows/win32/procthread/job-objects)
- [Windows reparse point operations](https://learn.microsoft.com/windows/win32/fileio/reparse-point-operations)
- [Windows TCP endpoint enumeration](https://learn.microsoft.com/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable)
- [SQLite online backup](https://sqlite.org/backup.html)
- [SQLite WAL](https://sqlite.org/wal.html)
- [GitHub-hosted runners](https://docs.github.com/actions/reference/runners/github-hosted-runners)
- [GitHub Actions hardening](https://docs.github.com/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions)
- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)

## Final verdict

**Approved with corrections.** Implementation may proceed against the reviewed
specification. Release readiness remains gated on executed application,
packaging, clean-machine, sanitization, licence, and publication checks.
