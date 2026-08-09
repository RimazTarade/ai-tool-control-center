# AI Tool Control Center

AI Tool Control Center is a privacy-first Windows desktop application for
discovering local AI tooling, reviewing every observation before import, and
validating narrowly scoped action previews through a Rust security boundary.
Action execution is not implemented in version 0.1.0.

This repository is an early `0.1.0` foundation. It is not a version 1.0
release and has no downloadable installer yet.

## What works

- local quick scans with incremental progress and cancellation;
- a mandatory discovery review queue backed by SQLite;
- separate installation, registration, enablement, runtime, connection,
  authentication, health, confidence, and review states;
- structured action validation with no shell interpolation;
- a Tauri 2 shell and React interface with a clearly labelled browser demo;
- a bounded, versioned Python scanner protocol;
- public-source and private-path repository checks.

## Build from source

Development requires Windows 11 x86-64, Rust 1.96, Node.js 24, pnpm 10, and
Python 3.14. End users of future release artifacts will not need these tools.

```powershell
pnpm install --frozen-lockfile
cargo test --workspace --locked
pnpm test
pnpm build
pnpm tauri build --no-bundle
```

## Privacy

Application-owned code sends no telemetry and performs no automatic outbound
requests. Discoveries remain local and pending until reviewed. Browser demo
data is synthetic and visibly labelled.

See the [reviewed specification](docs/superpowers/specs/2026-07-21-ai-tool-control-center-open-source-windows-design.md)
and [implementation plan](docs/superpowers/plans/2026-07-23-ai-tool-control-center-open-source-windows-implementation.md).

## License

Apache License 2.0. See `LICENSE`.
