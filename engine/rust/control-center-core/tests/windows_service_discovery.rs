use control_center_core::{
    Confidence, ObservedState, ToolKind,
    windows::{ServiceRecord, ServiceRuntimeState, ServiceSource, discover_services_with},
};

struct FixtureServices {
    fail: bool,
}

impl ServiceSource for FixtureServices {
    fn read_services(&self) -> Result<Vec<ServiceRecord>, String> {
        if self.fail {
            return Err("service enumeration fixture failed".into());
        }

        Ok(vec![
            ServiceRecord {
                service_name: "OllamaService".into(),
                display_name: Some("Ollama Service".into()),
                runtime_state: ServiceRuntimeState::Running,
            },
            ServiceRecord {
                service_name: "ollamaservice".into(),
                display_name: Some("Duplicate Ollama Service".into()),
                runtime_state: ServiceRuntimeState::Running,
            },
            ServiceRecord {
                service_name: "n8n-agent".into(),
                display_name: None,
                runtime_state: ServiceRuntimeState::Stopped,
            },
            ServiceRecord {
                service_name: "   ".into(),
                display_name: Some("Blank Service".into()),
                runtime_state: ServiceRuntimeState::Unknown,
            },
        ])
    }
}

#[test]
fn service_discovery_marks_registered_services_and_preserves_runtime_state() {
    let source = FixtureServices { fail: false };

    let mut discoveries = discover_services_with(&source).unwrap();
    discoveries.sort_by(|left, right| left.suggested_name.cmp(&right.suggested_name));

    assert_eq!(discoveries.len(), 2);
    assert_eq!(discoveries[0].suggested_name, "Ollama Service");
    assert_eq!(discoveries[1].suggested_name, "n8n-agent");

    assert_eq!(discoveries[0].runtime_state, ObservedState::Running);
    assert_eq!(discoveries[1].runtime_state, ObservedState::Stopped);

    for discovery in discoveries {
        assert_eq!(discovery.suggested_type, ToolKind::WindowsService);
        assert_eq!(discovery.source_scanner, "windows.service");
        assert_eq!(discovery.confidence, Confidence::High);
        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.registration_state, ObservedState::Registered);
        assert_eq!(discovery.evidence.len(), 1);
        assert_eq!(discovery.evidence[0].kind, "service");
        assert_eq!(discovery.fingerprint.len(), 64);
    }
}

#[test]
fn service_discovery_reports_service_source_failure() {
    let source = FixtureServices { fail: true };

    let error = discover_services_with(&source).unwrap_err();

    assert!(error.contains("service enumeration fixture failed"));
}
