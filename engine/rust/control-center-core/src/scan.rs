use crate::{Discovery, Evidence};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use walkdir::{DirEntry, WalkDir};

const EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "target",
    "AppData",
    "Cache",
    "Caches",
    ".cache",
    "cache2",
    "Code Cache",
    "GPUCache",
    "INetCache",
    "LocalCache",
    "Crashpad",
    "CrashDumps",
    "Temp",
    "tmp",
    "npm-cache",
    "pip-cache",
];
const SIGNALS: &[&str] = &[
    "mcp",
    "claude",
    "codex",
    "ollama",
    "openwebui",
    "n8n",
    "docker",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanEvent {
    Progress {
        visited: u64,
    },
    Discovery {
        discovery: Discovery,
    },
    ScannerFailed {
        scanner_id: String,
        code: String,
        message: String,
    },
    Completed {
        visited: u64,
        discovered: u64,
    },
    Cancelled {
        visited: u64,
        discovered: u64,
    },
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Debug)]
struct ScanSettlement {
    pending: std::collections::HashSet<String>,
}

impl ScanSettlement {
    fn new<I, S>(scanner_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            pending: scanner_ids.into_iter().map(Into::into).collect(),
        }
    }

    fn mark_settled(&mut self, scanner_id: &str) -> bool {
        self.pending.remove(scanner_id)
    }

    fn is_terminal(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Debug)]
enum ScannerTerminal {
    Completed { visited: u64, discovered: u64 },
    Cancelled { visited: u64, discovered: u64 },
    Failed { code: String, message: String },
}

#[derive(Debug)]
struct ScanCoordinatorState {
    settlement: ScanSettlement,
    visited: u64,
    discovered: u64,
    cancelled: bool,
}

impl ScanCoordinatorState {
    fn new<I, S>(scanner_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            settlement: ScanSettlement::new(scanner_ids),
            visited: 0,
            discovered: 0,
            cancelled: false,
        }
    }

    fn settle(&mut self, scanner_id: &str, terminal: ScannerTerminal) -> Vec<ScanEvent> {
        if !self.settlement.mark_settled(scanner_id) {
            return Vec::new();
        }

        let mut events = Vec::new();

        match terminal {
            ScannerTerminal::Completed {
                visited,
                discovered,
            } => {
                self.visited = self.visited.saturating_add(visited);
                self.discovered = self.discovered.saturating_add(discovered);
            }
            ScannerTerminal::Cancelled {
                visited,
                discovered,
            } => {
                self.visited = self.visited.saturating_add(visited);
                self.discovered = self.discovered.saturating_add(discovered);
                self.cancelled = true;
            }
            ScannerTerminal::Failed { code, message } => {
                events.push(ScanEvent::ScannerFailed {
                    scanner_id: scanner_id.to_string(),
                    code,
                    message,
                });
            }
        }

        if self.is_terminal() {
            events.push(if self.cancelled {
                ScanEvent::Cancelled {
                    visited: self.visited,
                    discovered: self.discovered,
                }
            } else {
                ScanEvent::Completed {
                    visited: self.visited,
                    discovered: self.discovered,
                }
            });
        }

        events
    }

    fn is_terminal(&self) -> bool {
        self.settlement.is_terminal()
    }
}

pub async fn quick_scan(
    roots: Vec<PathBuf>,
    events: mpsc::Sender<ScanEvent>,
    cancellation: CancellationToken,
) {
    const SCANNER_ID: &str = "filesystem.quick";

    let terminal_events = events.clone();
    let mut coordinator = ScanCoordinatorState::new([SCANNER_ID]);
    let task = tokio::task::spawn_blocking(move || scan_blocking(roots, events, cancellation));

    let terminal = match task.await {
        Ok(terminal) => terminal,
        Err(_) => ScannerTerminal::Failed {
            code: "scanner_failed".into(),
            message: "The filesystem scanner stopped unexpectedly".into(),
        },
    };

    for event in coordinator.settle(SCANNER_ID, terminal) {
        let _ = terminal_events.send(event).await;
    }
}

fn scan_blocking(
    roots: Vec<PathBuf>,
    events: mpsc::Sender<ScanEvent>,
    cancellation: CancellationToken,
) -> ScannerTerminal {
    let mut visited = 0;
    let mut discovered = 0;
    for root in roots {
        if cancellation.is_cancelled() {
            return ScannerTerminal::Cancelled {
                visited,
                discovered,
            };
        }
        if !root.is_absolute() || !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(5)
            .into_iter()
            .filter_entry(is_allowed)
            .filter_map(Result::ok)
        {
            if cancellation.is_cancelled() {
                return ScannerTerminal::Cancelled {
                    visited,
                    discovered,
                };
            }
            visited += 1;
            if visited % 64 == 0 {
                let _ = events.blocking_send(ScanEvent::Progress { visited });
            }
            if entry.file_type().is_file() && looks_relevant(entry.path()) {
                discovered += 1;
                let path = entry.path().to_path_buf();
                let mut discovery = Discovery::unknown(
                    entry.file_name().to_string_lossy(),
                    "filesystem.quick",
                    fingerprint(&path),
                );
                discovery.evidence.push(Evidence {
                    kind: "path".into(),
                    summary: path.display().to_string(),
                });
                let _ = events.blocking_send(ScanEvent::Discovery { discovery });
            }
        }
    }
    ScannerTerminal::Completed {
        visited,
        discovered,
    }
}

fn is_allowed(entry: &DirEntry) -> bool {
    entry.depth() == 0
        || !EXCLUDED.iter().any(|excluded| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(excluded)
        })
}

fn looks_relevant(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    SIGNALS.iter().any(|signal| name.contains(signal))
        && matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("json" | "yaml" | "yml" | "toml" | "exe" | "cmd" | "bat" | "ps1")
        )
}

fn fingerprint(path: &Path) -> String {
    Sha256::digest(path.to_string_lossy().to_ascii_lowercase().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn finds_generic_signal_and_finishes() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("example-mcp.json"), "{}").unwrap();
        let (sender, mut receiver) = mpsc::channel(16);
        quick_scan(
            vec![root.path().to_path_buf()],
            sender,
            CancellationToken::new(),
        )
        .await;
        let mut found = false;
        while let Some(event) = receiver.recv().await {
            match event {
                ScanEvent::Discovery { .. } => found = true,
                ScanEvent::Completed { discovered, .. } => {
                    assert_eq!(discovered, 1);
                    break;
                }
                _ => {}
            }
        }
        assert!(found);
    }
    #[test]
    fn scanner_failed_events_preserve_scanner_id() {
        let encoded = r#"{
        "kind": "scanner_failed",
        "scanner_id": "filesystem.quick",
        "code": "access_denied",
        "message": "partial failure"
    }"#;

        let event: ScanEvent = serde_json::from_str(encoded).unwrap();
        let round_trip = serde_json::to_value(event).unwrap();

        assert_eq!(
            round_trip
                .get("scanner_id")
                .and_then(|value| value.as_str()),
            Some("filesystem.quick")
        );
    }

    #[test]
    fn scanner_failure_is_isolated_until_other_scanners_settle() {
        let mut coordinator = ScanCoordinatorState::new(["windows.process", "windows.path"]);

        let failure_events = coordinator.settle(
            "windows.process",
            ScannerTerminal::Failed {
                code: "access_denied".into(),
                message: "partial failure".into(),
            },
        );

        assert!(matches!(
            failure_events.as_slice(),
            [ScanEvent::ScannerFailed {
                scanner_id,
                code,
                ..
            }] if scanner_id == "windows.process" && code == "access_denied"
        ));
        assert!(!coordinator.is_terminal());

        let completion_events = coordinator.settle(
            "windows.path",
            ScannerTerminal::Completed {
                visited: 3,
                discovered: 1,
            },
        );

        assert!(matches!(
            completion_events.as_slice(),
            [ScanEvent::Completed {
                visited: 3,
                discovered: 1,
                ..
            }]
        ));
        assert!(coordinator.is_terminal());
    }

    #[test]
    fn scan_settlement_waits_for_every_selected_scanner() {
        let mut settlement =
            ScanSettlement::new(["windows.path", "windows.process", "windows.services"]);

        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.path");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.path");
        settlement.mark_settled("unknown");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.process");
        assert!(!settlement.is_terminal());

        settlement.mark_settled("windows.services");
        assert!(settlement.is_terminal());
    }

    #[tokio::test]
    async fn skips_high_noise_cache_directories() {
        let root = tempdir().unwrap();
        let cache = root.path().join("Cache");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("example-mcp.json"), "{}").unwrap();
        let (sender, mut receiver) = mpsc::channel(16);
        quick_scan(
            vec![root.path().to_path_buf()],
            sender,
            CancellationToken::new(),
        )
        .await;

        while let Some(event) = receiver.recv().await {
            match event {
                ScanEvent::Discovery { .. } => panic!("cache discovery must be excluded"),
                ScanEvent::Completed { discovered, .. } => {
                    assert_eq!(discovered, 0);
                    break;
                }
                _ => {}
            }
        }
    }
}
