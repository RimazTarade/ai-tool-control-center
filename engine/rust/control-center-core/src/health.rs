use crate::{HealthCheckDefinition, HealthResult, HealthState};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

pub fn is_result_stale(
    definition: &HealthCheckDefinition,
    result: &HealthResult,
    now: DateTime<Utc>,
) -> bool {
    let Some(ttl_seconds) = definition.stale_after_seconds else {
        return false;
    };

    let age_seconds = now.signed_duration_since(result.observed_at).num_seconds();
    age_seconds > 0 && i128::from(age_seconds) > i128::from(ttl_seconds)
}

pub fn aggregate_health(
    definitions: &[HealthCheckDefinition],
    latest_results: &HashMap<Uuid, HealthResult>,
    now: DateTime<Utc>,
) -> HealthState {
    let required: Vec<&HealthCheckDefinition> = definitions
        .iter()
        .filter(|definition| definition.required)
        .collect();

    if required.is_empty() {
        return HealthState::Grey;
    }

    let mut required_state = HealthState::Green;
    let mut yellow = false;

    for definition in required {
        let Some(result) = latest_results.get(&definition.id) else {
            required_state = stronger_required(required_state, HealthState::Grey);
            continue;
        };

        match result.state {
            HealthState::Red => {
                required_state = stronger_required(required_state, HealthState::Red)
            }
            HealthState::Blue => {
                required_state = stronger_required(required_state, HealthState::Blue)
            }
            HealthState::Grey => {
                required_state = stronger_required(required_state, HealthState::Grey)
            }
            HealthState::Green => {
                if is_result_stale(definition, result, now) {
                    yellow = true;
                }
            }
            HealthState::Yellow => {
                required_state = stronger_required(required_state, HealthState::Grey)
            }
        }
    }

    for definition in definitions.iter().filter(|definition| !definition.required) {
        if let Some(result) = latest_results.get(&definition.id)
            && (result.state != HealthState::Green || is_result_stale(definition, result, now))
        {
            yellow = true;
        }
    }

    match required_state {
        HealthState::Red => HealthState::Red,
        HealthState::Blue => HealthState::Blue,
        HealthState::Grey => HealthState::Grey,
        HealthState::Yellow => HealthState::Yellow,
        HealthState::Green if yellow => HealthState::Yellow,
        HealthState::Green => HealthState::Green,
    }
}

fn stronger_required(current: HealthState, candidate: HealthState) -> HealthState {
    fn rank(state: &HealthState) -> u8 {
        match state {
            HealthState::Red => 5,
            HealthState::Blue => 4,
            HealthState::Grey => 3,
            HealthState::Yellow => 2,
            HealthState::Green => 1,
        }
    }

    if rank(&candidate) > rank(&current) {
        candidate
    } else {
        current
    }
}

use crate::{ConfigFormat, HealthCheckType, HealthTarget, redaction::redact};
use std::{net::IpAddr, path::Path, time::Duration};

pub const MAX_CONFIG_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthProbeOutcome {
    pub state: HealthState,
    pub result_code: String,
    pub redacted_evidence: String,
}

pub trait HealthProbeIo: Send + Sync {
    fn path_exists(&self, path: &Path) -> Result<bool, String>;
    fn file_exists(&self, path: &Path) -> Result<bool, String>;
    fn read_config(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, String>;
    fn process_running(&self, executable_path: &Path) -> Result<bool, String>;
    fn windows_service_running(&self, service_name: &str) -> Result<Option<bool>, String>;
    fn loopback_port_reachable(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeHealthProbeIo;

impl HealthProbeIo for NativeHealthProbeIo {
    fn path_exists(&self, path: &Path) -> Result<bool, String> {
        match std::fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    fn file_exists(&self, path: &Path) -> Result<bool, String> {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    fn read_config(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
        use std::io::Read;

        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;

        if bytes.len() > max_bytes {
            return Err("config exceeds maximum read size".to_string());
        }

        Ok(bytes)
    }

    fn process_running(&self, executable_path: &Path) -> Result<bool, String> {
        #[cfg(windows)]
        {
            use crate::windows::{ProcessSource, WindowsProcessSource, windows_path_key};

            let target = windows_path_key(executable_path);
            let processes = WindowsProcessSource.read_processes()?;
            Ok(processes.into_iter().any(|process| {
                process
                    .executable_path
                    .as_deref()
                    .is_some_and(|path| windows_path_key(path) == target)
            }))
        }

        #[cfg(not(windows))]
        {
            let _ = executable_path;
            Err("Windows process inspection is unavailable".to_string())
        }
    }

    fn windows_service_running(&self, service_name: &str) -> Result<Option<bool>, String> {
        #[cfg(windows)]
        {
            use crate::windows::{ServiceRuntimeState, ServiceSource, WindowsServiceSource};

            let services = WindowsServiceSource.read_services()?;
            Ok(services
                .into_iter()
                .find(|service| service.service_name.eq_ignore_ascii_case(service_name))
                .and_then(|service| match service.runtime_state {
                    ServiceRuntimeState::Running => Some(true),
                    ServiceRuntimeState::Stopped => Some(false),
                    ServiceRuntimeState::Unknown => None,
                }))
        }

        #[cfg(not(windows))]
        {
            let _ = service_name;
            Err("Windows service inspection is unavailable".to_string())
        }
    }

    fn loopback_port_reachable(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<bool, String> {
        let ip = host
            .parse::<IpAddr>()
            .map_err(|_| "loopback host must be a literal IP address".to_string())?;
        if !ip.is_loopback() {
            return Err("non-loopback socket targets are not permitted".to_string());
        }
        let address = std::net::SocketAddr::new(ip, port);
        match std::net::TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                drop(stream);
                Ok(true)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

pub fn execute_health_check(
    definition: &HealthCheckDefinition,
    io: &dyn HealthProbeIo,
) -> HealthProbeOutcome {
    match (&definition.check_type, &definition.target_metadata) {
        (HealthCheckType::PathExists, HealthTarget::Path { path }) => bool_probe(
            io.path_exists(path),
            "path_missing",
            "path exists",
            "path is missing",
        ),
        (HealthCheckType::FileExists, HealthTarget::Path { path }) => bool_probe(
            io.file_exists(path),
            "file_missing",
            "file exists",
            "file is missing",
        ),
        (HealthCheckType::ConfigPresent, HealthTarget::Config { path, .. }) => {
            match io.file_exists(path) {
                Ok(true) => outcome(HealthState::Green, "ok", "config is present"),
                Ok(false) => outcome(HealthState::Blue, "setup_required", "config is missing"),
                Err(error) => probe_error(error),
            }
        }
        (
            HealthCheckType::ConfigParses,
            HealthTarget::Config {
                path,
                format: ConfigFormat::Json,
            },
        ) => match io.file_exists(path) {
            Ok(false) => outcome(HealthState::Blue, "setup_required", "config is missing"),
            Err(error) => probe_error(error),
            Ok(true) => match io.read_config(path, MAX_CONFIG_BYTES) {
                Err(error) => probe_error(error),
                Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                    Ok(_) => outcome(HealthState::Green, "ok", "config parses as JSON"),
                    Err(_) => outcome(
                        HealthState::Red,
                        "config_invalid_json",
                        "config does not parse as JSON",
                    ),
                },
            },
        },
        (HealthCheckType::ProcessRunning, HealthTarget::ProcessExecutable { path }) => bool_probe(
            io.process_running(path),
            "process_not_running",
            "process is running",
            "process is not running",
        ),
        (HealthCheckType::WindowsServiceState, HealthTarget::WindowsService { service_name }) => {
            match io.windows_service_running(service_name) {
                Ok(Some(true)) => outcome(HealthState::Green, "ok", "service is running"),
                Ok(Some(false)) => {
                    outcome(HealthState::Red, "service_stopped", "service is stopped")
                }
                Ok(None) => outcome(
                    HealthState::Grey,
                    "service_state_unknown",
                    "service state is unknown",
                ),
                Err(error) => probe_error(error),
            }
        }
        (HealthCheckType::LoopbackPortReachable, HealthTarget::LoopbackSocket { host, port }) => {
            let Ok(ip) = host.parse::<IpAddr>() else {
                return unsupported("loopback host must be a literal IP address");
            };
            if !ip.is_loopback() {
                return unsupported("non-loopback socket targets are not permitted");
            }

            let timeout = Duration::from_millis(definition.timeout_ms.max(1));
            bool_probe(
                io.loopback_port_reachable(host, *port, timeout),
                "loopback_unreachable",
                "loopback port is reachable",
                "loopback port is unreachable",
            )
        }
        _ => unsupported("health check target does not match check type"),
    }
}

fn bool_probe(
    result: Result<bool, String>,
    false_code: &str,
    true_evidence: &str,
    false_evidence: &str,
) -> HealthProbeOutcome {
    match result {
        Ok(true) => outcome(HealthState::Green, "ok", true_evidence),
        Ok(false) => outcome(HealthState::Red, false_code, false_evidence),
        Err(error) => probe_error(error),
    }
}

fn probe_error(error: String) -> HealthProbeOutcome {
    outcome(
        HealthState::Grey,
        "health_probe_error",
        &format!("health probe failed: {error}"),
    )
}

fn unsupported(evidence: &str) -> HealthProbeOutcome {
    outcome(HealthState::Grey, "health_check_unsupported", evidence)
}

fn outcome(state: HealthState, result_code: &str, evidence: &str) -> HealthProbeOutcome {
    HealthProbeOutcome {
        state,
        result_code: result_code.to_string(),
        redacted_evidence: redact(evidence),
    }
}

#[cfg(test)]
mod tests {
    use super::{aggregate_health, is_result_stale};
    use crate::{
        HealthCheckDefinition, HealthCheckType, HealthExpectedCondition, HealthResult, HealthState,
        HealthTarget,
    };
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use std::{collections::HashMap, path::PathBuf};
    use uuid::Uuid;

    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 23, 7, 0, 0).single().unwrap()
    }

    fn definition_with_id(
        definition_id: Uuid,
        check_type: HealthCheckType,
        required: bool,
    ) -> HealthCheckDefinition {
        HealthCheckDefinition {
            id: definition_id,
            installation_id: id(100),
            check_type,
            required,
            expected_condition: HealthExpectedCondition::Exists,
            timeout_ms: 1_000,
            stale_after_seconds: None,
            target_metadata: HealthTarget::Path {
                path: PathBuf::from(r"C:\Tools\example.exe"),
            },
            created_at: now(),
            updated_at: now(),
        }
    }

    fn required_definition(check_type: HealthCheckType) -> HealthCheckDefinition {
        required_definition_with_id(id(1), check_type)
    }

    fn required_definition_with_id(
        definition_id: Uuid,
        check_type: HealthCheckType,
    ) -> HealthCheckDefinition {
        definition_with_id(definition_id, check_type, true)
    }

    fn optional_definition_with_id(
        definition_id: Uuid,
        check_type: HealthCheckType,
    ) -> HealthCheckDefinition {
        definition_with_id(definition_id, check_type, false)
    }

    fn result_for(definition_id: Uuid, state: HealthState, result_code: &str) -> HealthResult {
        HealthResult {
            id: Uuid::new_v4(),
            run_id: id(200),
            installation_id: id(100),
            check_definition_id: definition_id,
            check_type: HealthCheckType::FileExists,
            required: true,
            state,
            result_code: result_code.into(),
            redacted_evidence: String::new(),
            duration_ms: 1,
            observed_at: now(),
        }
    }

    #[test]
    fn health_aggregate_no_completed_required_checks_is_grey() {
        let definitions = vec![required_definition(HealthCheckType::FileExists)];
        assert_eq!(
            aggregate_health(&definitions, &HashMap::new(), now()),
            HealthState::Grey
        );
    }

    #[test]
    fn health_aggregate_one_required_pass_cannot_hide_missing_required_result() {
        let definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::FileExists),
            required_definition_with_id(id(2), HealthCheckType::ProcessRunning),
        ];
        let latest = HashMap::from([(id(1), result_for(id(1), HealthState::Green, "ok"))]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Grey
        );
    }

    #[test]
    fn health_aggregate_required_failure_is_red() {
        let definitions = vec![required_definition_with_id(
            id(1),
            HealthCheckType::FileExists,
        )];
        let latest = HashMap::from([(id(1), result_for(id(1), HealthState::Red, "file_missing"))]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Red
        );
    }

    #[test]
    fn health_aggregate_required_setup_state_is_blue() {
        let definitions = vec![required_definition_with_id(
            id(1),
            HealthCheckType::ConfigPresent,
        )];
        let latest = HashMap::from([(
            id(1),
            result_for(id(1), HealthState::Blue, "setup_required"),
        )]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Blue
        );
    }

    #[test]
    fn health_aggregate_required_unknown_is_grey() {
        let definitions = vec![required_definition_with_id(
            id(1),
            HealthCheckType::WindowsServiceState,
        )];
        let latest = HashMap::from([(
            id(1),
            result_for(id(1), HealthState::Grey, "service_state_unknown"),
        )]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Grey
        );
    }

    #[test]
    fn health_aggregate_optional_failure_is_yellow_not_red() {
        let definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::FileExists),
            optional_definition_with_id(id(2), HealthCheckType::ConfigParses),
        ];
        let latest = HashMap::from([
            (id(1), result_for(id(1), HealthState::Green, "ok")),
            (
                id(2),
                result_for(id(2), HealthState::Red, "config_invalid_json"),
            ),
        ]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Yellow
        );
    }

    #[test]
    fn health_aggregate_green_requires_every_required_check_to_pass() {
        let definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::FileExists),
            required_definition_with_id(id(2), HealthCheckType::ProcessRunning),
        ];
        let latest = HashMap::from([
            (id(1), result_for(id(1), HealthState::Green, "ok")),
            (id(2), result_for(id(2), HealthState::Green, "ok")),
        ]);

        assert_eq!(
            aggregate_health(&definitions, &latest, now()),
            HealthState::Green
        );
    }

    #[test]
    fn health_aggregate_precedence_is_red_blue_grey_yellow_green() {
        let red_blue_definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::FileExists),
            required_definition_with_id(id(2), HealthCheckType::ConfigPresent),
        ];
        let red_blue_latest = HashMap::from([
            (id(1), result_for(id(1), HealthState::Red, "file_missing")),
            (
                id(2),
                result_for(id(2), HealthState::Blue, "setup_required"),
            ),
        ]);
        assert_eq!(
            aggregate_health(&red_blue_definitions, &red_blue_latest, now()),
            HealthState::Red
        );

        let blue_grey_definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::ConfigPresent),
            required_definition_with_id(id(2), HealthCheckType::WindowsServiceState),
        ];
        let blue_grey_latest = HashMap::from([
            (
                id(1),
                result_for(id(1), HealthState::Blue, "setup_required"),
            ),
            (
                id(2),
                result_for(id(2), HealthState::Grey, "service_state_unknown"),
            ),
        ]);
        assert_eq!(
            aggregate_health(&blue_grey_definitions, &blue_grey_latest, now()),
            HealthState::Blue
        );

        let grey_yellow_definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::WindowsServiceState),
            optional_definition_with_id(id(2), HealthCheckType::ConfigParses),
        ];
        let grey_yellow_latest = HashMap::from([
            (
                id(1),
                result_for(id(1), HealthState::Grey, "service_state_unknown"),
            ),
            (
                id(2),
                result_for(id(2), HealthState::Red, "config_invalid_json"),
            ),
        ]);
        assert_eq!(
            aggregate_health(&grey_yellow_definitions, &grey_yellow_latest, now()),
            HealthState::Grey
        );

        let yellow_green_definitions = vec![
            required_definition_with_id(id(1), HealthCheckType::FileExists),
            optional_definition_with_id(id(2), HealthCheckType::ConfigParses),
        ];
        let yellow_green_latest = HashMap::from([
            (id(1), result_for(id(1), HealthState::Green, "ok")),
            (
                id(2),
                result_for(id(2), HealthState::Red, "config_invalid_json"),
            ),
        ]);
        assert_eq!(
            aggregate_health(&yellow_green_definitions, &yellow_green_latest, now()),
            HealthState::Yellow
        );
    }

    #[test]
    fn health_aggregate_stale_required_pass_becomes_yellow() {
        let mut definition = required_definition_with_id(id(1), HealthCheckType::FileExists);
        definition.stale_after_seconds = Some(60);

        let mut result = result_for(id(1), HealthState::Green, "ok");
        result.observed_at = now() - Duration::seconds(61);
        let latest = HashMap::from([(id(1), result)]);

        assert_eq!(
            aggregate_health(&[definition], &latest, now()),
            HealthState::Yellow
        );
    }

    #[test]
    fn health_aggregate_staleness_never_turns_red() {
        let mut definition = required_definition_with_id(id(1), HealthCheckType::FileExists);
        definition.stale_after_seconds = Some(1);

        let mut result = result_for(id(1), HealthState::Green, "ok");
        result.observed_at = now() - Duration::hours(1);
        let latest = HashMap::from([(id(1), result)]);

        let state = aggregate_health(&[definition], &latest, now());
        assert_eq!(state, HealthState::Yellow);
        assert_ne!(state, HealthState::Red);
    }

    #[test]
    fn health_aggregate_no_ttl_never_becomes_stale() {
        let definition = required_definition_with_id(id(1), HealthCheckType::FileExists);
        let mut result = result_for(id(1), HealthState::Green, "ok");
        result.observed_at = now() - Duration::days(3650);
        let latest = HashMap::from([(id(1), result)]);

        assert_eq!(
            aggregate_health(&[definition], &latest, now()),
            HealthState::Green
        );
    }

    #[test]
    fn health_result_stale_boundary_is_deterministic() {
        let mut definition = required_definition_with_id(id(1), HealthCheckType::FileExists);
        definition.stale_after_seconds = Some(60);

        let mut equal_boundary = result_for(id(1), HealthState::Green, "ok");
        equal_boundary.observed_at = now() - Duration::seconds(60);
        assert!(!is_result_stale(&definition, &equal_boundary, now()));

        let mut older_than_boundary = result_for(id(1), HealthState::Green, "ok");
        older_than_boundary.observed_at = now() - Duration::seconds(61);
        assert!(is_result_stale(&definition, &older_than_boundary, now()));
    }
}

#[cfg(test)]
mod probe_tests {
    use super::{HealthProbeIo, MAX_CONFIG_BYTES, NativeHealthProbeIo, execute_health_check};
    use crate::{
        ConfigFormat, HealthCheckDefinition, HealthCheckType, HealthExpectedCondition, HealthState,
        HealthTarget,
    };
    use chrono::{TimeZone, Utc};
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
        time::Duration,
    };
    use uuid::Uuid;

    struct FakeProbeIo {
        path_exists_result: Result<bool, String>,
        file_exists_result: Result<bool, String>,
        config_result: Result<Vec<u8>, String>,
        process_path: Option<PathBuf>,
        process_error: Option<String>,
        service_result: Result<Option<bool>, String>,
        loopback_result: Result<bool, String>,
        calls: Mutex<Vec<&'static str>>,
        max_read_seen: Mutex<Option<usize>>,
    }

    impl Default for FakeProbeIo {
        fn default() -> Self {
            Self {
                path_exists_result: Ok(false),
                file_exists_result: Ok(false),
                config_result: Ok(Vec::new()),
                process_path: None,
                process_error: None,
                service_result: Ok(None),
                loopback_result: Ok(false),
                calls: Mutex::new(Vec::new()),
                max_read_seen: Mutex::new(None),
            }
        }
    }

    impl HealthProbeIo for FakeProbeIo {
        fn path_exists(&self, _path: &Path) -> Result<bool, String> {
            self.calls.lock().unwrap().push("path_exists");
            self.path_exists_result.clone()
        }

        fn file_exists(&self, _path: &Path) -> Result<bool, String> {
            self.calls.lock().unwrap().push("file_exists");
            self.file_exists_result.clone()
        }

        fn read_config(&self, _path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
            self.calls.lock().unwrap().push("read_config");
            *self.max_read_seen.lock().unwrap() = Some(max_bytes);
            self.config_result.clone()
        }

        fn process_running(&self, executable_path: &Path) -> Result<bool, String> {
            self.calls.lock().unwrap().push("process_running");
            if let Some(error) = &self.process_error {
                return Err(error.clone());
            }
            Ok(self
                .process_path
                .as_deref()
                .is_some_and(|expected| expected == executable_path))
        }

        fn windows_service_running(&self, _service_name: &str) -> Result<Option<bool>, String> {
            self.calls.lock().unwrap().push("windows_service_running");
            self.service_result.clone()
        }

        fn loopback_port_reachable(
            &self,
            _host: &str,
            _port: u16,
            _timeout: Duration,
        ) -> Result<bool, String> {
            self.calls.lock().unwrap().push("loopback_port_reachable");
            self.loopback_result.clone()
        }
    }

    fn definition(
        check_type: HealthCheckType,
        expected_condition: HealthExpectedCondition,
        target_metadata: HealthTarget,
    ) -> HealthCheckDefinition {
        let timestamp = Utc.with_ymd_and_hms(2026, 8, 23, 8, 0, 0).single().unwrap();
        HealthCheckDefinition {
            id: Uuid::from_u128(1),
            installation_id: Uuid::from_u128(2),
            check_type,
            required: true,
            expected_condition,
            timeout_ms: 1_000,
            stale_after_seconds: None,
            target_metadata,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    fn path_target() -> HealthTarget {
        HealthTarget::Path {
            path: PathBuf::from(r"C:\Tools\example.exe"),
        }
    }

    fn config_target() -> HealthTarget {
        HealthTarget::Config {
            path: PathBuf::from(r"C:\Tools\config.json"),
            format: ConfigFormat::Json,
        }
    }

    #[test]
    fn health_probe_path_exists_pass_is_green() {
        let io = FakeProbeIo {
            path_exists_result: Ok(true),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::PathExists,
                HealthExpectedCondition::Exists,
                path_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Green);
        assert_eq!(outcome.result_code, "ok");
    }

    #[test]
    fn health_probe_path_exists_missing_is_red() {
        let io = FakeProbeIo::default();
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::PathExists,
                HealthExpectedCondition::Exists,
                path_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "path_missing");
    }

    #[test]
    fn health_probe_file_exists_checks_file_not_generic_path() {
        let io = FakeProbeIo {
            path_exists_result: Ok(true),
            file_exists_result: Ok(false),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::FileExists,
                HealthExpectedCondition::Exists,
                path_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "file_missing");
        assert_eq!(*io.calls.lock().unwrap(), vec!["file_exists"]);
    }

    #[test]
    fn health_probe_config_present_missing_is_blue_without_reading_contents() {
        let io = FakeProbeIo {
            file_exists_result: Ok(false),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ConfigPresent,
                HealthExpectedCondition::Exists,
                config_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Blue);
        assert_eq!(outcome.result_code, "setup_required");
        assert_eq!(*io.calls.lock().unwrap(), vec!["file_exists"]);
    }

    #[test]
    fn health_probe_config_parses_valid_json_is_green() {
        let io = FakeProbeIo {
            file_exists_result: Ok(true),
            config_result: Ok(br#"{"enabled":true}"#.to_vec()),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ConfigParses,
                HealthExpectedCondition::Parses,
                config_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Green);
        assert_eq!(outcome.result_code, "ok");
    }

    #[test]
    fn health_probe_config_parses_missing_is_blue() {
        let io = FakeProbeIo {
            file_exists_result: Ok(false),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ConfigParses,
                HealthExpectedCondition::Parses,
                config_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Blue);
        assert_eq!(outcome.result_code, "setup_required");
        assert_eq!(*io.calls.lock().unwrap(), vec!["file_exists"]);
    }

    #[test]
    fn health_probe_config_parses_invalid_json_is_red() {
        let io = FakeProbeIo {
            file_exists_result: Ok(true),
            config_result: Ok(b"{invalid json".to_vec()),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ConfigParses,
                HealthExpectedCondition::Parses,
                config_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "config_invalid_json");
    }

    #[test]
    fn health_probe_config_read_is_bounded_and_read_error_is_grey() {
        let io = FakeProbeIo {
            file_exists_result: Ok(true),
            config_result: Err("file exceeds configured read limit".into()),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ConfigParses,
                HealthExpectedCondition::Parses,
                config_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "health_probe_error");
        assert_eq!(*io.max_read_seen.lock().unwrap(), Some(MAX_CONFIG_BYTES));
    }

    #[test]
    fn health_probe_process_running_matches_exact_executable_path_not_name_similarity() {
        let io = FakeProbeIo {
            process_path: Some(PathBuf::from(r"C:\Tools\example.exe")),
            ..Default::default()
        };
        let exact = execute_health_check(
            &definition(
                HealthCheckType::ProcessRunning,
                HealthExpectedCondition::Running,
                HealthTarget::ProcessExecutable {
                    path: PathBuf::from(r"C:\Tools\example.exe"),
                },
            ),
            &io,
        );
        let same_name_elsewhere = execute_health_check(
            &definition(
                HealthCheckType::ProcessRunning,
                HealthExpectedCondition::Running,
                HealthTarget::ProcessExecutable {
                    path: PathBuf::from(r"C:\Other\example.exe"),
                },
            ),
            &io,
        );

        assert_eq!(exact.state, HealthState::Green);
        assert_eq!(exact.result_code, "ok");
        assert_eq!(same_name_elsewhere.state, HealthState::Red);
        assert_eq!(same_name_elsewhere.result_code, "process_not_running");
    }

    #[test]
    fn health_probe_process_missing_is_red() {
        let io = FakeProbeIo::default();
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::ProcessRunning,
                HealthExpectedCondition::Running,
                HealthTarget::ProcessExecutable {
                    path: PathBuf::from(r"C:\Tools\example.exe"),
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "process_not_running");
    }

    #[test]
    fn health_probe_service_running_is_green() {
        let io = FakeProbeIo {
            service_result: Ok(Some(true)),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::WindowsServiceState,
                HealthExpectedCondition::Running,
                HealthTarget::WindowsService {
                    service_name: "ExampleSvc".into(),
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Green);
        assert_eq!(outcome.result_code, "ok");
    }

    #[test]
    fn health_probe_service_stopped_is_red() {
        let io = FakeProbeIo {
            service_result: Ok(Some(false)),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::WindowsServiceState,
                HealthExpectedCondition::Running,
                HealthTarget::WindowsService {
                    service_name: "ExampleSvc".into(),
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "service_stopped");
    }

    #[test]
    fn health_probe_service_unknown_is_grey() {
        let io = FakeProbeIo {
            service_result: Ok(None),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::WindowsServiceState,
                HealthExpectedCondition::Running,
                HealthTarget::WindowsService {
                    service_name: "ExampleSvc".into(),
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "service_state_unknown");
    }

    #[test]
    fn health_probe_loopback_reachable_is_green() {
        let io = FakeProbeIo {
            loopback_result: Ok(true),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::LoopbackPortReachable,
                HealthExpectedCondition::Reachable,
                HealthTarget::LoopbackSocket {
                    host: "127.0.0.1".into(),
                    port: 8080,
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Green);
        assert_eq!(outcome.result_code, "ok");
        assert_eq!(*io.calls.lock().unwrap(), vec!["loopback_port_reachable"]);
    }

    #[test]
    fn health_probe_loopback_unreachable_is_red() {
        let io = FakeProbeIo::default();
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::LoopbackPortReachable,
                HealthExpectedCondition::Reachable,
                HealthTarget::LoopbackSocket {
                    host: "::1".into(),
                    port: 8080,
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Red);
        assert_eq!(outcome.result_code, "loopback_unreachable");
    }

    #[test]
    fn health_probe_nonloopback_is_rejected_before_socket_io() {
        let io = FakeProbeIo {
            loopback_result: Ok(true),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::LoopbackPortReachable,
                HealthExpectedCondition::Reachable,
                HealthTarget::LoopbackSocket {
                    host: "8.8.8.8".into(),
                    port: 53,
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "health_check_unsupported");
        assert!(io.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn health_probe_mismatched_target_is_grey_without_io() {
        let io = FakeProbeIo::default();
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::PathExists,
                HealthExpectedCondition::Exists,
                HealthTarget::ProcessExecutable {
                    path: PathBuf::from(r"C:\Tools\example.exe"),
                },
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "health_check_unsupported");
        assert!(io.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn health_probe_io_error_is_grey() {
        let io = FakeProbeIo {
            path_exists_result: Err("access denied".into()),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::PathExists,
                HealthExpectedCondition::Exists,
                path_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "health_probe_error");
    }

    #[test]
    fn health_probe_evidence_redacts_secrets() {
        let io = FakeProbeIo {
            path_exists_result: Err(
                "token=secret-token password=hunter2 Authorization: Bearer super-secret".into(),
            ),
            ..Default::default()
        };
        let outcome = execute_health_check(
            &definition(
                HealthCheckType::PathExists,
                HealthExpectedCondition::Exists,
                path_target(),
            ),
            &io,
        );

        assert_eq!(outcome.state, HealthState::Grey);
        assert_eq!(outcome.result_code, "health_probe_error");
        assert!(outcome.redacted_evidence.contains("token=[REDACTED]"));
        assert!(outcome.redacted_evidence.contains("password=[REDACTED]"));
        assert!(
            outcome
                .redacted_evidence
                .contains("Authorization: Bearer [REDACTED]")
        );
        assert!(!outcome.redacted_evidence.contains("secret-token"));
        assert!(!outcome.redacted_evidence.contains("hunter2"));
        assert!(!outcome.redacted_evidence.contains("super-secret"));
    }

    #[test]
    fn health_probe_native_io_implements_health_probe_io() {
        fn assert_impl<T: HealthProbeIo>() {}
        assert_impl::<NativeHealthProbeIo>();
    }

    // PROBE_TESTS_CONTINUE
}
