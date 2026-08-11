#![cfg(windows)]

use control_center_core::windows::{
    KnownLocationKind, windows_known_location_roots,
};

#[test]
fn native_known_location_roots_include_program_and_launcher_locations() {
    let report = windows_known_location_roots();

    assert!(
        report
            .roots
            .iter()
            .any(|root| root.kind == KnownLocationKind::Programs),
        "Windows known folders should expose at least one program root"
    );

    assert!(
        report
            .roots
            .iter()
            .any(|root| root.kind == KnownLocationKind::Launcher),
        "Windows known folders should expose at least one launcher root"
    );

    assert!(
        report.roots.iter().all(|root| root.path.is_absolute()),
        "native known-folder paths must be absolute"
    );

    assert!(
        report.roots.iter().all(|root| root.path.is_dir()),
        "returned native known-folder roots must exist as directories"
    );
}
