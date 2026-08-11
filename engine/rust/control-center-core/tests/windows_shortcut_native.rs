#![cfg(windows)]

use control_center_core::windows::{resolve_windows_shortcut, windows_path_key};
use std::{fs, process::Command};
use tempfile::tempdir;

#[test]
fn native_shortcut_resolution_reads_target_and_metadata() {
    let root = tempdir().expect("temporary root should be created");
    let target = root.path().join("Example Tool.exe");
    let workdir = root.path().join("Work");
    let shortcut = root.path().join("Example Launcher.lnk");

    fs::write(&target, b"fixture").unwrap();
    fs::create_dir_all(&workdir).unwrap();

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$shell = New-Object -ComObject WScript.Shell; $link = $shell.CreateShortcut($env:ATCC_SHORTCUT); $link.TargetPath = $env:ATCC_TARGET; $link.Arguments = '--fixture --mode=test'; $link.WorkingDirectory = $env:ATCC_WORKDIR; $link.Description = 'ATCC shortcut fixture'; $link.Save()",
        ])
        .env("ATCC_SHORTCUT", &shortcut)
        .env("ATCC_TARGET", &target)
        .env("ATCC_WORKDIR", &workdir)
        .status()
        .expect("PowerShell shortcut fixture command should run");

    assert!(status.success(), "shortcut fixture should be created");

    let metadata = resolve_windows_shortcut(&shortcut).expect("shortcut should resolve");

    assert_eq!(
        windows_path_key(&metadata.target_path),
        windows_path_key(&target)
    );
    assert_eq!(metadata.arguments.as_deref(), Some("--fixture --mode=test"));
    assert_eq!(
        metadata.working_directory.as_deref().map(windows_path_key),
        Some(windows_path_key(&workdir))
    );
    assert_eq!(
        metadata.description.as_deref(),
        Some("ATCC shortcut fixture")
    );
}
