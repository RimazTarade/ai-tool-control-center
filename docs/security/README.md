# Security model

The webview has no shell or unrestricted filesystem capability. Rust validates
structured action previews and rejects shell interpreters, community-defined
executables, and administrator requests in unsigned builds. Inventory-bound
confirmation, execution, and audit are roadmap work and are not exposed by the
0.1.0 desktop commands.
