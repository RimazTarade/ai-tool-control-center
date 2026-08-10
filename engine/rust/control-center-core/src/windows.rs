use crate::{Confidence, Discovery, Evidence, ToolKind};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// Returns a stable identity key for Windows paths.
///
/// Windows path identity is case-insensitive for the discovery purposes used
/// here. Both slash styles are normalized so observations from PATH, registry,
/// configuration files, and native APIs can be compared consistently.
pub fn windows_path_key(path: &Path) -> String {
    let mut key = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();

    while key.ends_with('\\') && !is_drive_root(&key) {
        key.pop();
    }

    key
}

/// Removes duplicate Windows paths while preserving the first observation.
pub fn dedupe_windows_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for path in paths {
        if seen.insert(windows_path_key(&path)) {
            deduped.push(path);
        }
    }

    deduped
}

/// Parses a semicolon-separated Windows PATH-style value into unique entries.
///
/// Surrounding whitespace is ignored, a single pair of surrounding quotes is
/// removed, and empty entries are discarded before Windows-style
/// case-insensitive deduplication.
pub fn parse_windows_path_entries(raw: &str) -> Vec<PathBuf> {
    let entries = raw.split(';').filter_map(|entry| {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return None;
        }

        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(trimmed);

        Some(PathBuf::from(unquoted.trim()))
    });

    dedupe_windows_paths(entries)
}

/// Observes executable files directly present in Windows PATH directories.
///
/// This is an observation scanner only. It does not execute discovered files
/// and every result remains pending for the mandatory review queue.
pub fn discover_path_executables(path_value: &str, pathext_value: &str) -> Vec<Discovery> {
    let executable_extensions: HashSet<String> = pathext_value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('.').to_ascii_lowercase())
        .collect();

    if executable_extensions.is_empty() {
        return Vec::new();
    }

    let mut discoveries = Vec::new();
    for directory in parse_windows_path_entries(path_value) {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() || !has_executable_extension(&path, &executable_extensions) {
                continue;
            }

            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };

            let mut discovery = Discovery::unknown(
                name,
                "windows.path",
                fingerprint_windows_path(&path),
            );
            discovery.suggested_type = ToolKind::Cli;
            discovery.confidence = Confidence::Medium;
            discovery.evidence.push(Evidence {
                kind: "path".into(),
                summary: path.display().to_string(),
            });
            discoveries.push(discovery);
        }
    }

    discoveries
}

fn has_executable_extension(path: &Path, extensions: &HashSet<String>) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| extensions.contains(&extension))
}

fn fingerprint_windows_path(path: &Path) -> String {
    Sha256::digest(windows_path_key(path).as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\'
}
