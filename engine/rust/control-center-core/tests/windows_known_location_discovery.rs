use std::fs;

use control_center_core::{
    Confidence, ToolKind,
    windows::{KnownLocationKind, KnownLocationRoot, discover_known_locations},
};
use tempfile::tempdir;

#[test]
fn known_location_discovery_is_bounded_excludes_noise_and_classifies_launchers() {
    let root = tempdir().expect("temporary root should be created");
    let programs = root.path().join("Programs");
    let vendor = programs.join("Vendor");
    let cache = programs.join("Cache");
    let too_deep = vendor.join("Nested").join("TooDeep");
    let launchers = root.path().join("StartMenu");

    fs::create_dir_all(&vendor).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&too_deep).unwrap();
    fs::create_dir_all(&launchers).unwrap();

    fs::write(vendor.join("ExampleTool.exe"), b"fixture").unwrap();
    fs::write(vendor.join("ExampleHelper.cmd"), b"fixture").unwrap();
    fs::write(vendor.join("README.txt"), b"fixture").unwrap();
    fs::write(cache.join("IgnoredTool.exe"), b"fixture").unwrap();
    fs::write(too_deep.join("TooDeep.exe"), b"fixture").unwrap();
    #[cfg(windows)]
    {
        let shortcut_target = vendor.join("ExampleTool.exe");
        let shortcut_path = launchers.join("Example Launcher.lnk");
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$shell = New-Object -ComObject WScript.Shell; $link = $shell.CreateShortcut($env:ATCC_SHORTCUT); $link.TargetPath = $env:ATCC_TARGET; $link.Save()",
            ])
            .env("ATCC_SHORTCUT", &shortcut_path)
            .env("ATCC_TARGET", &shortcut_target)
            .status()
            .expect("PowerShell shortcut fixture command should run");
        assert!(status.success(), "shortcut fixture should be created");
    }

    #[cfg(not(windows))]
    fs::write(launchers.join("Example Launcher.lnk"), b"fixture").unwrap();

    let roots = vec![
        KnownLocationRoot {
            kind: KnownLocationKind::Programs,
            path: programs,
        },
        KnownLocationRoot {
            kind: KnownLocationKind::Launcher,
            path: launchers,
        },
        KnownLocationRoot {
            kind: KnownLocationKind::Programs,
            path: root.path().join("Missing"),
        },
    ];

    let report = discover_known_locations(&roots, 2);

    assert!(report.errors.is_empty());
    assert_eq!(report.discoveries.len(), 3);

    let application = report
        .discoveries
        .iter()
        .find(|discovery| discovery.suggested_name == "ExampleTool")
        .expect("known executable should be discovered");
    assert_eq!(application.suggested_type, ToolKind::DesktopApplication);
    assert_eq!(application.source_scanner, "windows.known_location");
    assert_eq!(application.confidence, Confidence::Medium);
    assert_eq!(application.evidence.len(), 1);
    assert_eq!(application.evidence[0].kind, "path");
    assert_eq!(application.fingerprint.len(), 64);

    let helper = report
        .discoveries
        .iter()
        .find(|discovery| discovery.suggested_name == "ExampleHelper")
        .expect("command launcher should be discovered");
    assert_eq!(helper.suggested_type, ToolKind::Cli);

    let shortcut = report
        .discoveries
        .iter()
        .find(|discovery| discovery.suggested_name == "Example Launcher")
        .expect("shortcut launcher should be discovered");
    assert_eq!(shortcut.suggested_type, ToolKind::Launcher);

    assert!(
        report
            .discoveries
            .iter()
            .all(|discovery| discovery.suggested_name != "IgnoredTool")
    );
    assert!(
        report
            .discoveries
            .iter()
            .all(|discovery| discovery.suggested_name != "TooDeep")
    );
}

#[test]
fn known_location_discovery_keeps_valid_results_when_one_root_is_invalid() {
    let root = tempdir().expect("temporary root should be created");
    let valid = root.path().join("Programs");
    fs::create_dir_all(&valid).unwrap();
    fs::write(valid.join("ValidTool.exe"), b"fixture").unwrap();

    let invalid = root.path().join("not-a-directory.txt");
    fs::write(&invalid, b"fixture").unwrap();

    let report = discover_known_locations(
        &[
            KnownLocationRoot {
                kind: KnownLocationKind::Programs,
                path: invalid.clone(),
            },
            KnownLocationRoot {
                kind: KnownLocationKind::Programs,
                path: valid,
            },
        ],
        2,
    );

    assert_eq!(report.discoveries.len(), 1);
    assert_eq!(report.discoveries[0].suggested_name, "ValidTool");
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].root, invalid);
}

#[cfg(windows)]
#[test]
fn known_location_discovery_refuses_directory_junction_targets() {
    use std::process::Command;

    let root = tempdir().expect("temporary root should be created");
    let programs = root.path().join("Programs");
    let local = programs.join("Local");
    let outside = root.path().join("OutsideTarget");
    let junction = programs.join("Linked");

    fs::create_dir_all(&local).unwrap();
    fs::create_dir_all(&outside).unwrap();

    fs::write(local.join("VisibleTool.exe"), b"fixture").unwrap();
    fs::write(outside.join("HiddenBehindJunction.exe"), b"fixture").unwrap();

    let status = Command::new("cmd.exe")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(&outside)
        .status()
        .expect("junction creation command should run");

    assert!(status.success(), "junction fixture should be created");

    let report = discover_known_locations(
        &[KnownLocationRoot {
            kind: KnownLocationKind::Programs,
            path: programs,
        }],
        4,
    );

    let _ = Command::new("cmd.exe")
        .arg("/c")
        .arg("rmdir")
        .arg(&junction)
        .status();

    assert!(
        report
            .discoveries
            .iter()
            .any(|discovery| discovery.suggested_name == "VisibleTool")
    );
    assert!(
        report
            .discoveries
            .iter()
            .all(|discovery| discovery.suggested_name != "HiddenBehindJunction"),
        "scanner must never traverse a Windows directory junction"
    );
}
