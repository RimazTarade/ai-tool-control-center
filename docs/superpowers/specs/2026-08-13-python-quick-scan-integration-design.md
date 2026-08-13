# Python Quick Scan Integration Design

**Date:** 2026-08-13
**Branch:** `feat/0.2-python-quick-scan-integration`
**Status:** Approved

## Purpose

Integrate the existing bundled Python scanner into normal Quick Scan as the stable scanner `python.config`. Python must use the existing ScannerJob coordinator and the normal discovery persistence and review pipeline.

## Architecture

Quick Scan gains a `QuickScanContext` containing scan roots and a fallible Python application-root result, conceptually `python_app_root: Result<PathBuf, PythonRootError>`. The Tauri shell attempts runtime-root resolution because it owns application path knowledge, but resolution failure must be carried into the context rather than rejecting Quick Scan. The Rust core then settles `python.config` independently while continuing the other scanners.

Quick Scan will contain eight stable scanners: `filesystem.quick`, `windows.known_location`, `windows.path`, `windows.uninstall_registry`, `windows.process`, `windows.service`, `windows.tcp`, and `python.config`.

`python.config` is a normal ScannerJob. It uses the existing maximum concurrency of three, per-scanner timeout, cancellation hierarchy, progress events, panic isolation and settlement logic. It must not bypass the coordinator.

## Runtime Resolution

Development builds use the repository root containing `runtimes/`, derived deterministically from the compile-time Cargo manifest location rather than the current working directory. Packaged builds use the installed application root derived from the running executable location.

The supervisor must resolve only `<app_root>\runtimes\cpython-3.14.7-windows-x86_64\python.exe`. No PATH lookup, ambient Python, `python`, `python3`, `py`, PYTHONHOME or PYTHONPATH fallback is permitted.

## Failure Policy

Python failure is isolated. If runtime resolution fails or bundled Python is unavailable, only `python.config` fails. The seven native scanners continue. Stable supervisor codes remain `scanner_protocol`, `scanner_timeout`, `scanner_cancelled` and `scanner_failed`.

The overall Quick Scan reaches terminal state only after all eight scanner jobs complete, fail or cancel.

## Data Flow

Tauri start_quick_scan -> QuickScanContext -> build_quick_scan_jobs -> python.config -> existing Python supervisor -> bundled python.exe -> ai_tool_control_scanner -> translated Discovery -> ScanEvent::Discovery -> existing Tauri persistence -> pending Review Queue.

Python receives the same Quick Scan roots already selected by the application. Runtime location and discovery scope remain separate concepts.

## Security

The existing Python supervisor remains the security boundary. It continues using `-I -m ai_tool_control_scanner`, bounded stdin/stdout/stderr, redacted stderr and Windows process-tree cleanup.

## TDD

Implementation starts with failing tests proving: `python.config` is a stable scanner; exactly one job is built for each stable ID; Python discoveries become normal ScanEvent::Discovery events; Python discoveries contribute to final totals; missing runtime fails only python.config; native scanners continue after Python failure; final settlement still occurs; development and packaged roots resolve deterministically and absolutely.

Existing supervisor tests for protocol bounds, redaction, timeout, cancellation and descendant cleanup must not be duplicated.

## Acceptance Criteria

The slice is complete when python.config runs through the bounded coordinator, bundled CPython is the only interpreter used, Python discoveries use the existing persistence path, missing runtime affects only python.config, cancellation and timeout preserve normal settlement, final state waits for all scanners and no ambient Python fallback exists.

## Verification

Run Rust formatting, Clippy, full workspace tests, Python Ruff, mypy and unittest, frontend checks, Tauri release compile, repository policy, git diff check and bundled-runtime smoke. Perform real Windows acceptance once with the runtime present and once with it temporarily unavailable.

## Scope Boundary

Stop after Python Quick Scan integration and acceptance. Do not include frontend redesign, final release packaging or the 0.2 version bump.
