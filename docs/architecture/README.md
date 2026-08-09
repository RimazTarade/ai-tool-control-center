# Architecture

React renders local state and invokes a small set of typed Tauri commands.
Rust owns SQLite, the current filesystem scan, cancellation, and the review
boundary. A bounded JSON Lines Python scanner is developed and tested in this
repository, but version 0.1.0 does not yet bundle or supervise it as a private
sidecar. Discoveries enter SQLite as pending and cannot become inventory
without a review decision.
