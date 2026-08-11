use control_center_core::{
    Confidence, ObservedState, ToolKind, windows::discover_path_executables,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn path_executable_discovery_emits_cli_discoveries_for_pathext_files_only() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("ollama.exe"), b"fixture").unwrap();
    fs::write(root.path().join("codex.cmd"), b"fixture").unwrap();
    fs::write(root.path().join("notes.txt"), b"fixture").unwrap();

    let path_value = format!("{};{}", root.path().display(), root.path().display());
    let mut discoveries = discover_path_executables(&path_value, ".EXE;.CMD");
    discoveries.sort_by(|left, right| left.suggested_name.cmp(&right.suggested_name));

    assert_eq!(discoveries.len(), 2);
    assert_eq!(discoveries[0].suggested_name, "codex");
    assert_eq!(discoveries[1].suggested_name, "ollama");

    for discovery in discoveries {
        assert_eq!(discovery.suggested_type, ToolKind::Cli);
        assert_eq!(discovery.source_scanner, "windows.path");
        assert_eq!(discovery.confidence, Confidence::Medium);
        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.evidence.len(), 1);
        assert_eq!(discovery.evidence[0].kind, "path");
        assert_eq!(discovery.fingerprint.len(), 64);
    }
}

#[test]
fn path_executable_discovery_ignores_missing_path_directories() {
    let root = tempdir().unwrap();
    let missing = root.path().join("does-not-exist");

    let discoveries = discover_path_executables(&missing.display().to_string(), ".EXE;.CMD");

    assert!(discoveries.is_empty());
}
