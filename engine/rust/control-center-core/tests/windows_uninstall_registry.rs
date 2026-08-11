use control_center_core::{
    Confidence, ObservedState, ToolKind,
    windows::{
        RegistryHive, RegistryView, UninstallRegistryRecord, UninstallRegistrySource,
        discover_uninstall_registry_with,
    },
};
use std::{cell::RefCell, path::PathBuf};

#[derive(Default)]
struct FixtureRegistry {
    visited: RefCell<Vec<(RegistryHive, RegistryView)>>,
}

impl UninstallRegistrySource for FixtureRegistry {
    fn read_uninstall_entries(
        &self,
        hive: RegistryHive,
        view: RegistryView,
    ) -> Result<Vec<UninstallRegistryRecord>, String> {
        self.visited.borrow_mut().push((hive, view));

        match (hive, view) {
            (RegistryHive::CurrentUser, RegistryView::Registry64) => Ok(vec![
                UninstallRegistryRecord {
                    hive,
                    view,
                    key_name: "Ollama".into(),
                    display_name: Some("Ollama".into()),
                    install_location: Some(PathBuf::from(r"C:\Fixture\Programs\Ollama")),
                    publisher: Some("Ollama".into()),
                },
                UninstallRegistryRecord {
                    hive,
                    view,
                    key_name: "Nameless".into(),
                    display_name: Some("   ".into()),
                    install_location: None,
                    publisher: None,
                },
            ]),
            (RegistryHive::LocalMachine, RegistryView::Registry64) => {
                Ok(vec![UninstallRegistryRecord {
                    hive,
                    view,
                    key_name: "{DOCKER-FIXTURE}".into(),
                    display_name: Some("Docker Desktop".into()),
                    install_location: Some(PathBuf::from(r"C:\Program Files\Docker\Docker")),
                    publisher: Some("Docker Inc.".into()),
                }])
            }
            (RegistryHive::LocalMachine, RegistryView::Registry32) => {
                Err("access denied fixture".into())
            }
            (RegistryHive::CurrentUser, RegistryView::Registry32) => Ok(Vec::new()),
        }
    }
}

#[test]
fn uninstall_registry_discovery_scans_all_hives_and_views_without_aborting_on_one_error() {
    let registry = FixtureRegistry::default();

    let report = discover_uninstall_registry_with(&registry);

    let visited = registry.visited.borrow();
    assert_eq!(visited.len(), 4);
    assert!(visited.contains(&(RegistryHive::CurrentUser, RegistryView::Registry64)));
    assert!(visited.contains(&(RegistryHive::CurrentUser, RegistryView::Registry32)));
    assert!(visited.contains(&(RegistryHive::LocalMachine, RegistryView::Registry64)));
    assert!(visited.contains(&(RegistryHive::LocalMachine, RegistryView::Registry32)));

    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].hive, RegistryHive::LocalMachine);
    assert_eq!(report.errors[0].view, RegistryView::Registry32);
    assert!(report.errors[0].message.contains("access denied fixture"));
}

#[test]
fn uninstall_registry_discovery_emits_reviewable_desktop_applications_and_skips_blank_names() {
    let registry = FixtureRegistry::default();

    let mut discoveries = discover_uninstall_registry_with(&registry).discoveries;
    discoveries.sort_by(|left, right| left.suggested_name.cmp(&right.suggested_name));

    assert_eq!(discoveries.len(), 2);
    assert_eq!(discoveries[0].suggested_name, "Docker Desktop");
    assert_eq!(discoveries[1].suggested_name, "Ollama");

    for discovery in discoveries {
        assert_eq!(discovery.suggested_type, ToolKind::DesktopApplication);
        assert_eq!(discovery.source_scanner, "windows.uninstall_registry");
        assert_eq!(discovery.confidence, Confidence::High);
        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.evidence.len(), 1);
        assert_eq!(discovery.evidence[0].kind, "registry");
        assert_eq!(discovery.fingerprint.len(), 64);
    }
}
