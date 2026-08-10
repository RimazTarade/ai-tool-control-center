#![cfg(windows)]

use control_center_core::windows::{ProcessSource, WindowsProcessSource};

#[test]
fn native_process_source_includes_the_current_process() {
    let source = WindowsProcessSource;

    let records = source
        .read_processes()
        .expect("native Windows process enumeration should succeed");

    assert!(!records.is_empty());
    assert!(
        records
            .iter()
            .all(|record| record.pid > 0 && !record.name.trim().is_empty())
    );

    let current_pid = std::process::id();
    let current = records
        .iter()
        .find(|record| record.pid == current_pid)
        .expect("the current test process should be visible in the Windows process snapshot");

    assert!(
        current
            .executable_path
            .as_ref()
            .is_some_and(|path| path.is_absolute())
    );
}
