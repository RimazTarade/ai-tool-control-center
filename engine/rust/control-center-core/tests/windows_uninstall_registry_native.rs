#![cfg(windows)]

use control_center_core::windows::{
    RegistryHive, RegistryView, UninstallRegistrySource, WindowsUninstallRegistrySource,
};

#[test]
fn native_uninstall_registry_source_reads_each_requested_registry_view() {
    let source = WindowsUninstallRegistrySource;

    for (hive, view) in [
        (RegistryHive::CurrentUser, RegistryView::Registry64),
        (RegistryHive::CurrentUser, RegistryView::Registry32),
        (RegistryHive::LocalMachine, RegistryView::Registry64),
        (RegistryHive::LocalMachine, RegistryView::Registry32),
    ] {
        let records = source
            .read_uninstall_entries(hive, view)
            .unwrap_or_else(|message| {
                panic!("failed to read {hive:?} {view:?} uninstall registry view: {message}")
            });

        assert!(records.iter().all(|record| {
            record.hive == hive && record.view == view && !record.key_name.trim().is_empty()
        }));
    }
}
