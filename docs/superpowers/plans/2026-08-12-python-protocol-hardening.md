# Python Scanner Protocol Hardening Implementation Plan

Goal: Finish Python protocol hardening, then add evidence-based Claude, Codex, MCP, and Docker interpretation.

Architecture: Keep the existing Python JSON Lines contract stable. Python owns bounded framing, request validation, redaction, and product interpretation. Rust will later own the shared DiscoveryEnvelope, bundled-runtime supervision, timeouts, cancellation, and Windows process-tree cleanup.

Tech stack: CPython 3.14 standard library, unittest, Ruff, mypy, uv.

## Task 1: Complete Protocol Boundary Hardening

Files:
- engine/python/src/ai_tool_control_scanner/protocol.py
- engine/python/src/ai_tool_control_scanner/__main__.py
- engine/python/tests/test_scanner.py

Steps:
1. Preserve the already-green oversized-input and malformed-JSON recovery tests.
2. Add invalid UTF-8 recovery coverage.
3. Add an exact protocol-version test proving JSON true is rejected instead of being treated as integer 1.
4. Tighten protocol-version validation.
5. Add explicit invalid request-shape coverage.
6. Add bounded-output coverage.
7. Run the full Python test file.
8. Run Ruff and mypy.
9. Commit the protocol-hardening slice separately.

## Task 2: Add Evidence-Based Product Interpretation

Files:
- engine/python/src/ai_tool_control_scanner/scanners.py
- engine/python/tests/test_scanner.py

Steps:
1. Add Claude configuration classification without health claims.
2. Add Codex canonical-directory detection.
3. Add JSON and TOML mapping parsing using the standard library only.
4. Preserve generic MCP detection.
5. Preserve product identity when MCP mappings appear inside Claude or Codex configuration.
6. Add Docker configuration evidence without runtime or authentication claims.
7. Add a false-positive guard for unrelated configuration files.
8. Run all Python tests.
9. Run Ruff and mypy.
10. Commit the interpreter slice separately.

## Task 3: Verify the Python Slice

Steps:
1. Run Ruff with the exact Milestone 5 command.
2. Run mypy with the exact Milestone 5 command.
3. Run the full unittest discovery command.
4. Run git diff --check.
5. Confirm the working tree only contains intentional changes.
6. Confirm Python did not take ownership of scan_id, scanner_id, sequence, observed_at, hard timeout, process-tree cancellation, or bundled-runtime supervision.
7. Stop before the Rust supervisor and bundled CPython runtime slice.
