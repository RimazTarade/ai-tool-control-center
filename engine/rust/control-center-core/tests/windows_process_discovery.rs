use control_center_core::{
    Confidence, ObservedState, ToolKind,
    windows::{ProcessRecord, ProcessSource, discover_processes_with},
};
use std::path::PathBuf;

struct FixtureProcesses {
    fail: bool,
}

impl ProcessSource for FixtureProcesses {
    fn read_processes(&self) -> Result<Vec<ProcessRecord>, String> {
        if self.fail {
            return Err("process enumeration fixture failed".into());
        }

        Ok(vec![
            ProcessRecord {
                pid: 101,
                name: "ollama.exe".into(),
                executable_path: Some(PathBuf::from(r"C:\Fixture\Ollama\ollama.exe")),
            },
            ProcessRecord {
                pid: 202,
                name: "OLLAMA.EXE".into(),
                executable_path: Some(PathBuf::from(r"c:/fixture/ollama/OLLAMA.EXE")),
            },
            ProcessRecord {
                pid: 303,
                name: "codex.exe".into(),
                executable_path: None,
            },
            ProcessRecord {
                pid: 404,
                name: "   ".into(),
                executable_path: None,
            },
        ])
    }
}

#[test]
fn process_discovery_marks_running_processes_and_deduplicates_executable_identity() {
    let source = FixtureProcesses { fail: false };

    let mut discoveries = discover_processes_with(&source).unwrap();
    discoveries.sort_by(|left, right| left.suggested_name.cmp(&right.suggested_name));

    assert_eq!(discoveries.len(), 2);
    assert_eq!(discoveries[0].suggested_name, "codex");
    assert_eq!(discoveries[1].suggested_name, "ollama");

    assert_eq!(discoveries[0].confidence, Confidence::Medium);
    assert_eq!(discoveries[1].confidence, Confidence::High);

    for discovery in discoveries {
        assert_eq!(discovery.suggested_type, ToolKind::Unknown);
        assert_eq!(discovery.source_scanner, "windows.process");
        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.runtime_state, ObservedState::Running);
        assert_eq!(discovery.evidence.len(), 1);
        assert_eq!(discovery.evidence[0].kind, "process");
        assert_eq!(discovery.fingerprint.len(), 64);
    }
}

#[test]
fn process_discovery_reports_process_source_failure() {
    let source = FixtureProcesses { fail: true };

    let error = discover_processes_with(&source).unwrap_err();

    assert!(error.contains("process enumeration fixture failed"));
}
