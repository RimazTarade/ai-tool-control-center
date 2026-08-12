# Python Scanner Protocol Hardening Design

Date: 2026-08-12
Status: Approved

## Scope

This slice hardens the bundled Python scanner process without prematurely moving Rust-owned orchestration responsibilities into Python.

The existing JSON Lines request and response shape remains stable while protocol robustness and product-specific interpretation are improved.

## Protocol ownership

Python continues to accept requests containing:

- `protocol_version`
- `request_id`
- `operation`
- `roots`

Supported operations remain:

- `scan`
- `cancel`
- `ping`

Python continues to emit the existing response kinds:

- `pong`
- `cancelled`
- `discovery`
- `completed`
- `error`

The Python process does not yet emit the complete language-neutral `DiscoveryEnvelope`.

The future Rust supervisor owns:

- `scan_id`
- stable outer `scanner_id`
- monotonic `sequence`
- `observed_at`
- final enum validation
- final timestamp validation
- translation of Python discoveries into the shared `DiscoveryEnvelope`

This avoids coupling the Python interpreter layer to scan orchestration responsibilities.

## Input hardening

The Python supervisor reads stdin through the binary buffer with a hard 1 MiB message limit.

An oversized line is drained through its newline or EOF without retaining the whole line in memory. The supervisor reports `message_too_large` and continues processing subsequent messages.

Malformed JSON reports `malformed_json` without terminating the supervisor.

Invalid UTF-8 reports `invalid_utf8` without terminating the supervisor.

Protocol version validation must reject values that are not exactly integer version `1`.

Requests must reject structurally invalid required fields using stable protocol errors.

## Output and error handling

JSON Lines output remains bounded to the existing 1 MiB protocol message limit.

Unexpected scanner exceptions are converted into `scanner_failed` responses rather than terminating the process.

Error messages exposed through the protocol are redacted before output.

The Rust supervisor will later impose independent stdout and stderr bounds around the child process.

## Product-specific interpretation

After protocol hardening, the Python scanner adds focused interpretation for:

- Claude
- Codex
- common MCP configuration
- Docker and relevant CLI-derived evidence where appropriate

Interpretation must remain evidence-based.

The scanner must not infer healthy, authenticated, connected, enabled, or running states merely from configuration-file existence.

Missing or incomplete evidence remains unknown.

## Cancellation boundary

The current Python process remains sequential and does not claim responsive in-flight cancellation through stdin.

Hard timeout, cancellation, child-process termination, and Windows process-tree cleanup belong to the Rust supervisor using the owned bundled interpreter process.

## Testing

Protocol tests cover:

- malformed JSON
- oversized input
- recovery after malformed input
- recovery after oversized input
- invalid UTF-8
- exact protocol-version validation
- request-shape validation
- redaction
- bounded output

Interpreter tests cover representative Claude, Codex, MCP, Docker, and generic configuration fixtures.

Tests must confirm that evidence is discovered without manufacturing health claims.

## Deferred work

The following remain explicitly deferred to the Rust supervisor slice:

- complete `DiscoveryEnvelope` construction
- sequence-number enforcement
- RFC3339 timestamp validation
- shared enum validation
- absolute bundled-interpreter launch
- environment isolation
- stdout and stderr process bounds
- hard timeout
- cancellation
- Windows Job Object ownership and descendant cleanup
- staged-runtime smoke testing

## Success criteria

This slice is complete when Python lint, strict typing, and unit tests pass and the process survives malformed, oversized, and otherwise invalid protocol messages without losing framing for subsequent valid requests.

The implementation must preserve the existing Python request and response contract until the Rust supervisor integration introduces the shared outer discovery envelope.
