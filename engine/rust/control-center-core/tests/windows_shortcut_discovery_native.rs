#![cfg(windows)]

use control_center_core::{
    ToolKind,
    windows::{KnownLocationKind, KnownLocationRoot, discover_known_locations, windows_path_key},
};
use std::{fs, process::Command};
use tempfile::tempdir;

#[test]
fn native_shortcut_discovery_includes_resolved_target_evidence() {
    let root = tempdir().expect("temporary root should be created");
    let launcher_root = root.path().join("Launchers");
    let target_root = root.path().join("Target");
    let target = target_root.join("Example Tool.exe");
    let shortcut = launcher_root.join("Example Launcher.lnk");

    fs::create_dir_all(&launcher_root).unwrap();
    fs::create_dir_all(&target_root).unwrap();
    fs::write(&target, b"fixture").unwrap();

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$shell = New-Object -ComObject WScript.Shell; $link = $shell.CreateShortcut($env:ATCC_SHORTCUT); $link.TargetPath = $env:ATCC_TARGET; $link.Save()",
        ])
        .env("ATCC_SHORTCUT", &shortcut)
        .env("ATCC_TARGET", &target)
        .status()
        .expect("PowerShell shortcut fixture command should run");

    assert!(status.success(), "shortcut fixture should be created");

    let report = discover_known_locations(
        &[KnownLocationRoot {
            kind: KnownLocationKind::Launcher,
            path: launcher_root,
        }],
        2,
    );

    let discovery = report
        .discoveries
        .iter()
        .find(|item| item.suggested_name == "Example Launcher")
        .expect("shortcut should produce a launcher discovery");

    assert_eq!(discovery.suggested_type, ToolKind::Launcher);

    let shortcut_evidence = discovery
        .evidence
        .iter()
        .find(|evidence| evidence.kind == "shortcut")
        .expect("launcher discovery should include resolved shortcut evidence");

    let expected_target = windows_path_key(&target);

    assert!(
    shortcut_evidence.summary.contains(&expected_target),
    "shortcut evidence mismatch\nexpected target: {expected_target}\nactual evidence: {}",
    shortcut_evidence.summary
    );
}
