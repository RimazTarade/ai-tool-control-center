use control_center_core::python_supervisor::resolve_staged_python;
use std::fs;
use tempfile::tempdir;

#[test]
fn resolves_only_the_app_owned_staged_python() {
    let root = tempdir().unwrap();
    let runtime = root
        .path()
        .join("runtimes")
        .join("cpython-3.14.7-windows-x86_64");

    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("python.exe"), b"fixture").unwrap();

    let resolved = resolve_staged_python(root.path()).unwrap();

    assert!(resolved.is_absolute());
    assert_eq!(resolved, runtime.join("python.exe"));
}

#[test]
fn launch_spec_is_isolated_from_ambient_python() {
    use control_center_core::python_supervisor::python_launch_spec;

    let root = tempdir().unwrap();
    let runtime = root
        .path()
        .join("runtimes")
        .join("cpython-3.14.7-windows-x86_64");

    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("python.exe"), b"fixture").unwrap();

    let spec = python_launch_spec(root.path()).unwrap();

    assert_eq!(spec.program, runtime.join("python.exe"));
    assert_eq!(spec.current_dir, runtime);
    assert_eq!(spec.args, ["-I", "-m", "ai_tool_control_scanner"]);

    for name in [
        "PATH",
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "PYTHONUSERBASE",
    ] {
        assert!(spec.removed_env.contains(&name));
    }
}

#[test]
fn encodes_bounded_scan_request_as_json_line() {
    use control_center_core::python_supervisor::encode_scan_request;

    let encoded = encode_scan_request("req-1", &["C:\\Users\\test\\.config".to_string()]).unwrap();

    assert!(encoded.ends_with('\n'));

    let value: serde_json::Value = serde_json::from_str(encoded.trim_end()).unwrap();
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(value["request_id"], "req-1");
    assert_eq!(value["operation"], "scan");
    assert_eq!(value["roots"][0], "C:\\Users\\test\\.config");
}

#[test]
fn rejects_scan_request_larger_than_one_mibibyte() {
    use control_center_core::python_supervisor::encode_scan_request;

    let oversized_root = "x".repeat(1_048_576);
    let result = encode_scan_request("req-oversized", &[oversized_root]);

    assert_eq!(result, Err("scanner_protocol"));
}

#[test]
fn supervisor_errors_expose_only_stable_public_codes() {
    use control_center_core::python_supervisor::PythonSupervisorError;

    assert_eq!(PythonSupervisorError::protocol().code(), "scanner_protocol");
    assert_eq!(PythonSupervisorError::timeout().code(), "scanner_timeout");
    assert_eq!(
        PythonSupervisorError::cancelled().code(),
        "scanner_cancelled"
    );
    assert_eq!(
        PythonSupervisorError::failed("safe detail").code(),
        "scanner_failed"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn public_scan_fails_closed_when_staged_runtime_is_missing() {
    use control_center_core::python_supervisor::run_python_scan;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    let root = tempdir().unwrap();

    let error = run_python_scan(
        root.path(),
        &[],
        Duration::from_secs(1),
        CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), "scanner_failed");
}
