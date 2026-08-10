use control_center_core::windows::{
    dedupe_windows_paths, parse_windows_path_entries, windows_path_key,
};
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

#[test]
fn windows_path_list_parser_trims_quotes_ignores_empty_entries_and_deduplicates() {
    let raw = r#" C:\Tools ; "C:\Program Files\Vendor" ; c:/tools/ ;; C:\Other "#;

    assert_eq!(
        parse_windows_path_entries(raw),
        vec![
            PathBuf::from(r"C:\Tools"),
            PathBuf::from(r"C:\Program Files\Vendor"),
            PathBuf::from(r"C:\Other"),
        ]
    );
}
