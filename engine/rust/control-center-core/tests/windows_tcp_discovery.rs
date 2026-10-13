use control_center_core::{
    Confidence, ObservedState, ToolKind,
    windows::{TcpEndpointRecord, TcpEndpointSource, discover_tcp_endpoints_with},
};

#[derive(Default)]
struct FixtureTcpEndpointSource {
    fail: bool,
}

impl TcpEndpointSource for FixtureTcpEndpointSource {
    fn read_listening_endpoints(&self) -> Result<Vec<TcpEndpointRecord>, String> {
        if self.fail {
            return Err("tcp endpoint fixture failed".into());
        }

        Ok(vec![
            TcpEndpointRecord {
                local_address: "127.0.0.1".into(),
                local_port: 11434,
                owning_pid: Some(4242),
            },
            TcpEndpointRecord {
                local_address: "127.0.0.1".into(),
                local_port: 11434,
                owning_pid: Some(9999),
            },
            TcpEndpointRecord {
                local_address: "::1".into(),
                local_port: 5678,
                owning_pid: None,
            },
            TcpEndpointRecord {
                local_address: "   ".into(),
                local_port: 3000,
                owning_pid: Some(1),
            },
            TcpEndpointRecord {
                local_address: "0.0.0.0".into(),
                local_port: 0,
                owning_pid: Some(2),
            },
        ])
    }
}

#[test]
fn tcp_endpoint_discovery_emits_running_local_services_and_deduplicates_endpoint_identity() {
    let discoveries = discover_tcp_endpoints_with(&FixtureTcpEndpointSource::default())
        .expect("fixture TCP endpoint discovery should succeed");

    assert_eq!(discoveries.len(), 2);

    let loopback = discoveries
        .iter()
        .find(|discovery| discovery.suggested_name == "127.0.0.1:11434")
        .expect("IPv4 listener should be discovered");
    assert_eq!(loopback.suggested_type, ToolKind::LocalService);
    assert_eq!(loopback.source_scanner, "windows.tcp");
    assert_eq!(loopback.confidence, Confidence::Medium);
    assert_eq!(loopback.installation_state, ObservedState::Detected);
    assert_eq!(loopback.runtime_state, ObservedState::Running);
    assert_eq!(loopback.registration_state, ObservedState::Unknown);
    assert_eq!(loopback.evidence.len(), 1);
    assert_eq!(loopback.evidence[0].kind, "tcp");
    assert!(loopback.evidence[0].summary.contains("pid=4242"));
    assert_eq!(loopback.fingerprint.len(), 64);

    let ipv6 = discoveries
        .iter()
        .find(|discovery| discovery.suggested_name == "[::1]:5678")
        .expect("IPv6 listener should be discovered");
    assert_eq!(ipv6.runtime_state, ObservedState::Running);
    assert!(ipv6.evidence[0].summary.contains("pid=unknown"));
}

#[test]
fn tcp_endpoint_discovery_reports_source_failure() {
    let source = FixtureTcpEndpointSource { fail: true };
    let error = discover_tcp_endpoints_with(&source).expect_err("fixture failure should propagate");

    assert_eq!(error, "tcp endpoint fixture failed");
}

struct TypedTcpEndpoints;

impl TcpEndpointSource for TypedTcpEndpoints {
    fn read_listening_endpoints(&self) -> Result<Vec<TcpEndpointRecord>, String> {
        Ok(vec![
            TcpEndpointRecord {
                local_address: "127.0.0.1".into(),
                local_port: 11434,
                owning_pid: Some(42),
            },
            TcpEndpointRecord {
                local_address: "192.168.1.50".into(),
                local_port: 8080,
                owning_pid: Some(84),
            },
        ])
    }
}

#[test]
fn typed_tcp_discovery_only_enriches_loopback_endpoints() {
    use control_center_core::{HealthCheckType, HealthExpectedCondition, HealthTarget};

    let discoveries = discover_tcp_endpoints_with(&TypedTcpEndpoints).unwrap();

    let loopback = discoveries
        .iter()
        .find(|item| item.suggested_name == "127.0.0.1:11434")
        .expect("loopback listener should be discovered");
    assert_eq!(
        loopback.canonical_identity.as_deref(),
        Some("loopback-tcp:127.0.0.1:11434")
    );
    assert_eq!(loopback.health_hints.len(), 1);
    let hint = &loopback.health_hints[0];
    assert_eq!(hint.check_type, HealthCheckType::LoopbackPortReachable);
    assert!(hint.required);
    assert_eq!(hint.expected_condition, HealthExpectedCondition::Reachable);
    assert_eq!(hint.stale_after_seconds, None);
    assert_eq!(
        hint.target,
        HealthTarget::LoopbackSocket {
            host: "127.0.0.1".into(),
            port: 11434
        }
    );

    let non_loopback = discoveries
        .iter()
        .find(|item| item.suggested_name == "192.168.1.50:8080")
        .expect("non-loopback listener should still be discovered");
    assert_eq!(non_loopback.canonical_identity, None);
    assert!(non_loopback.health_hints.is_empty());
}
