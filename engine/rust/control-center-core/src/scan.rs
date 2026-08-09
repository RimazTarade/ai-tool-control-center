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
    Progress { visited: u64 },
    Discovery { discovery: Discovery },
    ScannerFailed { code: String, message: String },
    Completed { visited: u64, discovered: u64 },
    Cancelled { visited: u64, discovered: u64 },
    Failed { code: String, message: String },
}

pub async fn quick_scan(
    roots: Vec<PathBuf>,
    events: mpsc::Sender<ScanEvent>,
    cancellation: CancellationToken,
) {
    let failure_events = events.clone();
    let task = tokio::task::spawn_blocking(move || scan_blocking(roots, events, cancellation));
    if task.await.is_err() {
        let _ = failure_events
            .send(ScanEvent::Failed {
                code: "scanner_failed".into(),
                message: "The filesystem scanner stopped unexpectedly".into(),
            })
            .await;
    }
}

fn scan_blocking(
    roots: Vec<PathBuf>,
    events: mpsc::Sender<ScanEvent>,
    cancellation: CancellationToken,
) {
    let mut visited = 0;
    let mut discovered = 0;
    for root in roots {
        if cancellation.is_cancelled() {
            let _ = events.blocking_send(ScanEvent::Cancelled {
                visited,
                discovered,
            });
            return;
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
                let _ = events.blocking_send(ScanEvent::Cancelled {
                    visited,
                    discovered,
                });
                return;
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
    let _ = events.blocking_send(ScanEvent::Completed {
        visited,
        discovered,
    });
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
