use std::{
    collections::HashSet,
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

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\'
}
