use control_center_core::windows::{dedupe_windows_paths, windows_path_key};
use std::path::PathBuf;

#[test]
fn windows_path_identity_is_case_insensitive_and_separator_stable() {
    assert_eq!(
        windows_path_key(&PathBuf::from(r"C:\Tools\\")),
        r"c:\tools"
    );
    assert_eq!(
        windows_path_key(&PathBuf::from("c:/tools")),
        r"c:\tools"
    );
}

#[test]
fn windows_path_deduplication_preserves_first_observation() {
    let entries = vec![
        PathBuf::from(r"C:\Tools"),
        PathBuf::from("c:/tools/"),
        PathBuf::from(r"C:\Other"),
        PathBuf::from(r"c:\OTHER\\"),
    ];

    assert_eq!(
        dedupe_windows_paths(entries),
        vec![PathBuf::from(r"C:\Tools"), PathBuf::from(r"C:\Other")]
    );
}
