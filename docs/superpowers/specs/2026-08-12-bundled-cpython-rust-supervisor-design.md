# Bundled CPython and Rust Supervisor Design

Date: 2026-08-12
Status: Approved

## Scope

This slice adds deterministic staging for an application-owned CPython runtime and a standalone Rust supervisor for the Python configuration scanner.

The Python-backed scanner is not wired into quick scan until containment, staging, protocol, timeout, cancellation and cleanup behavior are independently verified.

## Runtime pin

The bundled runtime is CPython 3.14.7 for Windows x86-64.

Artifact:
- python-3.14.7-embed-amd64.zip
- SHA-256: d297e5ff019966817ad8502465176139f2d3d840fa4ed84b13bed399a6ab1f15
- Staged path: runtimes/cpython-3.14.7-windows-x86_64/

The manifest contains CPython only in this slice. WebView2 remains deferred to the later packaging slice.

Runtime downloads occur only through an explicitly invoked packaging script. Application startup never downloads Python and never falls back to an ambient interpreter.

## Runtime staging architecture

`packaging/runtime-manifest.json` contains the pinned CPython runtime metadata for this slice:

- version
- architecture
- official artifact URL
- artifact filename
- SHA-256
- licence metadata

`packaging/fetch-runtimes.ps1` is explicitly invoked by the developer or packaging workflow. It does not run during normal application startup.

The script:

1. downloads the runtime to a temporary location
2. verifies SHA-256 before extraction
3. extracts into `runtimes/cpython-3.14.7-windows-x86_64/`
4. validates the expected embeddable-runtime layout
5. copies `ai_tool_control_scanner` into an application-owned directory inside the staged runtime
6. validates and configures the CPython `._pth` isolation file
7. exposes only the standard-library paths plus the staged scanner package
8. fails closed if the hash, archive layout or isolation file is unexpected

The generated `runtimes/` directory remains outside source control.

No runtime `pip` installation is performed. No ambient or user-installed Python interpreter is used.

## Rust supervisor architecture

A new focused core module owns the Python child process:

`engine/rust/control-center-core/src/python_supervisor.rs`

The supervisor launches only the absolute staged interpreter:

`runtimes/cpython-3.14.7-windows-x86_64/python.exe`

It never invokes `python`, `python3` or searches `PATH`.

The stable scanner ID is:

`python.config`

For each invocation, Rust:

1. creates one Python child process tree
2. sanitizes Python-specific environment variables including `PYTHONHOME` and `PYTHONPATH`
3. uses an application-controlled working directory
4. attaches the child to a kill-on-close Windows Job Object
5. sends one bounded JSON Lines request through stdin
6. reads stdout incrementally
7. enforces a 1 MiB maximum stdout line
8. enforces a 64 MiB total stdout limit per invocation
9. captures at most 256 KiB of stderr
10. redacts stderr and surfaced process errors
11. translates each Python discovery immediately into a Rust `Discovery`
12. treats malformed or oversized Python output as a scanner protocol failure
13. closes stdin on cancellation
14. allows up to two seconds for cooperative exit
15. closes the owned Job Object if the child remains alive
16. guarantees timeout, cancellation and child failure do not panic or block the wider scanner coordinator

A new focused module:

`engine/rust/control-center-core/src/redaction.rs`

owns reusable secret-shaped value redaction rather than embedding redaction logic inside process supervision.

## Discovery translation

The Python product labels map into truthful Rust categories:

- `mcp` -> `ToolKind::Mcp`
- `claude` -> `ToolKind::Configuration`
- `codex` -> `ToolKind::Configuration`
- `docker` -> `ToolKind::Configuration`
- `ollama` -> `ToolKind::Configuration`
- unknown values -> `ToolKind::Unknown`

Configuration evidence does not imply running, authenticated, connected, enabled or healthy state.

Rust owns the shared domain fields including `observed_at`, review state and unknown status dimensions.

## Testing and failure behavior

This slice is developed test-first and keeps `python.config` standalone until containment is proven.

Required coverage:

1. manifest parsing and immutable runtime metadata
2. SHA-256 mismatch rejection
3. unexpected archive or runtime layout rejection
4. deterministic staging only under ignored `runtimes/`
5. absolute owned-interpreter resolution without `PATH` lookup
6. staged-runtime smoke with `PYTHONHOME` and `PYTHONPATH` neutralized
7. successful `ping` -> `pong` handshake through the staged interpreter
8. incremental translation of multiple Python discoveries
9. 1 MiB stdout-line rejection
10. 64 MiB total stdout rejection
11. malformed child JSON rejection
12. 256 KiB stderr capture bound
13. secret redaction before scanner errors are surfaced
14. product-label to `ToolKind` mapping
15. hard-timeout termination
16. cancellation with a two-second cooperative-exit window
17. owned process-tree cleanup after timeout, cancellation and supervisor teardown

Public failure codes are:

- malformed or oversized child protocol -> `scanner_protocol`
- hard deadline exceeded -> `scanner_timeout`
- requested cancellation -> `scanner_cancelled`
- spawn, runtime or containment failure -> `scanner_failed`

Raw child stderr, secret-shaped values and uncontrolled environment paths are never surfaced directly.

## Integration boundary

This design stops after the standalone `python.config` scanner and staged runtime are verified.

Adding `python.config` to the quick-scan scanner list is a separate integration step after this containment slice is green.

WebView2 staging, installer packaging and portable ZIP construction remain deferred to the later packaging slice.
