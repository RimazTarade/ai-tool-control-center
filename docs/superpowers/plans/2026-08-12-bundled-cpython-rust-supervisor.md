# Bundled CPython and Rust Supervisor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

Goal: Stage a verified private CPython 3.14.7 runtime and build a standalone Rust supervisor for the `python.config` scanner.

Architecture: Packaging owns deterministic runtime fetch, hash verification, extraction and `._pth` isolation. Rust owns absolute-interpreter launch, bounded JSON Lines IPC, redaction, timeout, cancellation and Windows process-tree containment. Quick-scan integration remains deferred until the standalone scanner is verified.

Tech stack: Rust 1.96, Tokio, windows-sys/windows crates, PowerShell, CPython 3.14.7 embeddable x86-64.

## Global constraints

- CPython version: 3.14.7
- Artifact: python-3.14.7-embed-amd64.zip
- SHA-256: d297e5ff019966817ad8502465176139f2d3d840fa4ed84b13bed399a6ab1f15
- Staging path: runtimes/cpython-3.14.7-windows-x86_64/
- Scanner ID: python.config
- Maximum stdout line: 1 MiB
- Maximum total stdout: 64 MiB
- Maximum captured stderr: 256 KiB
- Cancellation grace period: 2 seconds
- No ambient Python fallback
- No runtime pip installation
- No quick-scan integration in this plan

## Task 1: Verified CPython Runtime Staging

Files:
- packaging/runtime-manifest.json
- packaging/fetch-runtimes.ps1
- engine/python/src/ai_tool_control_scanner/

Steps:
1. Add the CPython-only runtime manifest with the approved version, artifact name, official URL, architecture, SHA-256 and licence metadata.
2. Add a failing staging test or validation path for a wrong SHA-256.
3. Implement explicit runtime download to a temporary location.
4. Verify SHA-256 before extraction.
5. Extract only into runtimes/cpython-3.14.7-windows-x86_64/.
6. Validate the expected python.exe and CPython ._pth layout.
7. Copy ai_tool_control_scanner into the staged runtime.
8. Configure the ._pth file so only the embeddable standard-library paths plus the staged scanner package are available.
9. Fail closed on unexpected layout or isolation-file contents.
10. Run the staging script and verify the runtime remains under ignored runtimes/.
11. Commit the runtime-staging slice separately.

## Task 2: Rust Redaction and Python Discovery Translation

Files:
- engine/rust/control-center-core/src/redaction.rs
- engine/rust/control-center-core/src/python_supervisor.rs
- engine/rust/control-center-core/src/lib.rs
- engine/rust/control-center-core/tests/

Steps:
1. Add failing Rust tests for secret-shaped redaction.
2. Implement reusable redaction for authorization, API keys, tokens and passwords.
3. Add failing tests for Python discovery translation.
4. Parse the existing Python JSON Lines response shapes without changing the Python protocol.
5. Map mcp to ToolKind::Mcp.
6. Map claude, codex, docker and ollama to ToolKind::Configuration.
7. Map unknown labels to ToolKind::Unknown.
8. Preserve evidence, fingerprint, suggested name and confidence.
9. Keep runtime, authentication, connection, enablement and health states unknown.
10. Use python.config as the stable Rust source_scanner.
11. Reject malformed Python responses with scanner_protocol.
12. Run focused Rust tests and commit this slice separately.

## Task 3: Standalone Rust Python Process Supervisor

Files:
- engine/rust/control-center-core/src/python_supervisor.rs
- engine/rust/control-center-core/src/lib.rs
- engine/rust/control-center-core/Cargo.toml
- engine/rust/control-center-core/tests/

Steps:
1. Add a failing test proving the supervisor resolves an absolute staged python.exe and never searches PATH.
2. Add failing tests for sanitized PYTHONHOME and PYTHONPATH.
3. Spawn the staged interpreter with piped stdin, stdout and stderr.
4. Send one bounded JSON Lines request through stdin.
5. Stream stdout line-by-line with a 1 MiB per-line limit.
6. Enforce a 64 MiB total stdout limit per invocation.
7. Capture at most 256 KiB of stderr.
8. Redact stderr and surfaced child-process errors before returning failures.
9. Emit translated discoveries incrementally as stdout lines arrive.
10. Treat malformed or oversized output as scanner_protocol.
11. Add hard-timeout handling that returns scanner_timeout.
12. Add cancellation handling that closes stdin and allows a two-second cooperative exit window.
13. On Windows, attach the child to a kill-on-close Job Object and terminate the owned process tree when required.
14. Verify no child or descendant survives timeout, cancellation or supervisor teardown.
15. Run focused Rust tests and commit the supervisor slice separately.

## Task 4: Staged Runtime Smoke and Final Verification

Files:
- packaging/verify-artifacts.ps1
- packaging/runtime-manifest.json
- packaging/fetch-runtimes.ps1
- engine/rust/control-center-core/src/python_supervisor.rs
- engine/rust/control-center-core/tests/

Steps:
1. Add a staged-runtime smoke test that clears PYTHONHOME and PYTHONPATH and neutralizes developer Python paths.
2. Launch the absolute staged interpreter directly.
3. Verify ping returns pong through the real bundled runtime.
4. Verify no ambient Python fallback is required.
5. Run cargo fmt --all -- --check.
6. Run cargo clippy --workspace --all-targets --locked -- -D warnings.
7. Run cargo test --workspace --locked.
8. Run the Python Ruff, mypy and unittest gates.
9. Run the repository policy check.
10. Run git diff --check.
11. Confirm runtimes/ remains ignored and untracked.
12. Stop before adding python.config to the quick-scan job list.
