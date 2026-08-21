use serde::Deserialize;

use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonLaunchSpec {
    pub program: PathBuf,
    pub current_dir: PathBuf,
    pub args: Vec<&'static str>,
    pub removed_env: Vec<&'static str>,
}

pub fn python_launch_spec(app_root: &Path) -> Result<PythonLaunchSpec, &'static str> {
    let program = resolve_staged_python(app_root)?;
    let current_dir = program.parent().ok_or("scanner_failed")?.to_path_buf();

    Ok(PythonLaunchSpec {
        program,
        current_dir,
        args: vec!["-I", "-m", "ai_tool_control_scanner"],
        removed_env: vec![
            "PATH",
            "PYTHONHOME",
            "PYTHONPATH",
            "PYTHONSTARTUP",
            "PYTHONUSERBASE",
            "PYTHONINSPECT",
            "PYTHONWARNINGS",
            "PYTHONEXECUTABLE",
        ],
    })
}
pub fn resolve_staged_python(app_root: &Path) -> Result<PathBuf, &'static str> {
    let python = app_root
        .join("runtimes")
        .join("cpython-3.14.7-windows-x86_64")
        .join("python.exe");

    if !python.is_absolute() || !python.is_file() {
        return Err("scanner_failed");
    }

    Ok(python)
}

use crate::{Confidence, Discovery, Evidence, ToolKind};

const SCANNER_PROTOCOL: &str = "scanner_protocol";

const MAX_MESSAGE_BYTES: usize = 1_048_576;

pub fn encode_scan_request(request_id: &str, roots: &[String]) -> Result<String, &'static str> {
    let mut encoded = serde_json::to_string(&serde_json::json!({
        "protocol_version": 1,
        "request_id": request_id,
        "operation": "scan",
        "roots": roots,
    }))
    .map_err(|_| SCANNER_PROTOCOL)?;

    encoded.push('\n');

    if encoded.len() > MAX_MESSAGE_BYTES {
        return Err(SCANNER_PROTOCOL);
    }

    Ok(encoded)
}

const MAX_STDOUT_BYTES: usize = 64 * 1024 * 1024;

async fn read_bounded_line<R>(
    reader: &mut R,
    total_stdout: &mut usize,
) -> Result<Option<String>, &'static str>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();

    loop {
        let buffer = reader.fill_buf().await.map_err(|_| "scanner_failed")?;

        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(SCANNER_PROTOCOL)
            };
        }

        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(buffer.len(), |index| index + 1);

        let next_total = total_stdout.checked_add(take).ok_or(SCANNER_PROTOCOL)?;
        if next_total > MAX_STDOUT_BYTES {
            return Err(SCANNER_PROTOCOL);
        }

        if line.len().checked_add(take).ok_or(SCANNER_PROTOCOL)? > MAX_MESSAGE_BYTES {
            return Err(SCANNER_PROTOCOL);
        }

        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        *total_stdout = next_total;

        if newline.is_some() {
            line.pop();

            if line.last() == Some(&b'\r') {
                line.pop();
            }

            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| SCANNER_PROTOCOL);
        }
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
// SAFETY: Windows kernel handles are process-scoped. This wrapper uniquely owns the handle
// and moving ownership between threads does not duplicate or concurrently close it.
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn new() -> Result<Self, &'static str> {
        use std::mem::{size_of, zeroed};
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err("scanner_failed");
        }

        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };

        if configured == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return Err("scanner_failed");
        }

        Ok(Self { handle })
    }

    fn assign(&self, child: &tokio::process::Child) -> Result<(), &'static str> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        let process = child.raw_handle().ok_or("scanner_failed")?;

        if unsafe { AssignProcessToJobObject(self.handle, process.cast()) } == 0 {
            return Err("scanner_failed");
        }

        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Distinguishes a cooperative Python process-tree exit (the child settled
/// within the 2-second grace period after stdin was closed) from a forced
/// exit (the Windows Job Object had to be closed because the process tree
/// did not exit in time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonTermination {
    Cooperative,
    Forced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonSupervisorError {
    code: &'static str,
    termination: Option<PythonTermination>,
}

impl PythonSupervisorError {
    pub fn protocol() -> Self {
        Self {
            code: "scanner_protocol",
            termination: None,
        }
    }

    pub fn timeout() -> Self {
        Self {
            code: "scanner_timeout",
            termination: None,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            code: "scanner_cancelled",
            termination: None,
        }
    }

    pub fn cancelled_with(termination: PythonTermination) -> Self {
        Self {
            code: "scanner_cancelled",
            termination: Some(termination),
        }
    }

    pub fn failed(_detail: &str) -> Self {
        Self {
            code: "scanner_failed",
            termination: None,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn termination(&self) -> Option<PythonTermination> {
        self.termination
    }
}

async fn consume_python_stdout<R, F>(
    reader: &mut R,
    mut on_discovery: F,
) -> Result<u64, PythonSupervisorError>
where
    R: AsyncBufRead + Unpin,
    F: FnMut(Discovery),
{
    let mut total_stdout = 0usize;

    loop {
        let line = read_bounded_line(reader, &mut total_stdout)
            .await
            .map_err(|code| {
                if code == SCANNER_PROTOCOL {
                    PythonSupervisorError::protocol()
                } else {
                    PythonSupervisorError::failed(code)
                }
            })?
            .ok_or_else(PythonSupervisorError::protocol)?;

        let event = parse_python_response(&line).map_err(|_| PythonSupervisorError::protocol())?;

        match event {
            PythonScannerEvent::Discovery(discovery) => on_discovery(discovery),
            PythonScannerEvent::Completed { count } => return Ok(count),
            PythonScannerEvent::Cancelled => return Err(PythonSupervisorError::cancelled()),
            PythonScannerEvent::Error { code, .. } => {
                return Err(match code.as_str() {
                    "scanner_protocol" => PythonSupervisorError::protocol(),
                    "scanner_timeout" => PythonSupervisorError::timeout(),
                    "scanner_cancelled" => PythonSupervisorError::cancelled(),
                    _ => PythonSupervisorError::failed(&code),
                });
            }
            PythonScannerEvent::Pong => return Err(PythonSupervisorError::protocol()),
        }
    }
}

const MAX_STDERR_BYTES: usize = 256 * 1024;

async fn capture_stderr<R>(mut reader: R) -> Result<String, PythonSupervisorError>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::with_capacity(MAX_STDERR_BYTES);
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| PythonSupervisorError::failed("stderr read failed"))?;

        if read == 0 {
            break;
        }

        if captured.len() < MAX_STDERR_BYTES {
            let remaining = MAX_STDERR_BYTES - captured.len();
            let keep = read.min(remaining);
            captured.extend_from_slice(&buffer[..keep]);
        }
    }

    let text = String::from_utf8_lossy(&captured);
    Ok(crate::redaction::redact(&text))
}

#[cfg(windows)]
struct SupervisorControl {
    timeout: std::time::Duration,
    cancellation: tokio_util::sync::CancellationToken,
}

/// Called once root cancellation fires and the child's stdin has already
/// been closed (child stdin is shut down immediately after the scan
/// request is written, before either select loop below can observe
/// cancellation). Waits up to 2 seconds for the process tree to exit on
/// its own; if it does not, closes the Windows Job Object to force
/// termination and waits for the child to settle.
#[cfg(windows)]
async fn await_cancellation_grace_period(
    child: &mut tokio::process::Child,
    job: WindowsJob,
) -> PythonTermination {
    if tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
        .await
        .is_err()
    {
        drop(job);
        let _ = child.wait().await;
        PythonTermination::Forced
    } else {
        drop(job);
        PythonTermination::Cooperative
    }
}

#[cfg(windows)]
async fn run_supervised_process<F>(
    program: &Path,
    args: &[std::ffi::OsString],
    current_dir: &Path,
    removed_env: &[&str],
    request: &str,
    control: SupervisorControl,
    on_discovery: F,
) -> Result<u64, PythonSupervisorError>
where
    F: FnMut(Discovery),
{
    use std::process::Stdio;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for name in removed_env {
        command.env_remove(name);
    }

    let mut child = command
        .spawn()
        .map_err(|_| PythonSupervisorError::failed("spawn failed"))?;

    let job =
        WindowsJob::new().map_err(|_| PythonSupervisorError::failed("job creation failed"))?;

    job.assign(&child)
        .map_err(|_| PythonSupervisorError::failed("job assignment failed"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| PythonSupervisorError::failed("stdin unavailable"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PythonSupervisorError::failed("stdout unavailable"))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PythonSupervisorError::failed("stderr unavailable"))?;

    let stderr_task = tokio::spawn(capture_stderr(stderr));

    stdin
        .write_all(request.as_bytes())
        .await
        .map_err(|_| PythonSupervisorError::failed("stdin write failed"))?;

    stdin
        .shutdown()
        .await
        .map_err(|_| PythonSupervisorError::failed("stdin close failed"))?;

    drop(stdin);

    let mut stdout = BufReader::new(stdout);
    let consume = consume_python_stdout(&mut stdout, on_discovery);
    tokio::pin!(consume);
    let deadline = tokio::time::Instant::now() + control.timeout;

    let count = tokio::select! {
        result = &mut consume => result?,
        _ = control.cancellation.cancelled() => {
            let termination = await_cancellation_grace_period(&mut child, job).await;
            let _ = stderr_task.await;
            return Err(PythonSupervisorError::cancelled_with(termination));
        }
        _ = tokio::time::sleep_until(deadline) => {
            drop(job);
            let _ = child.wait().await;
            let _ = stderr_task.await;
            return Err(PythonSupervisorError::timeout());
        }
    };

    let status = tokio::select! {
        result = child.wait() => {
            result.map_err(|_| PythonSupervisorError::failed("child wait failed"))?
        }
        _ = control.cancellation.cancelled() => {
            let termination = await_cancellation_grace_period(&mut child, job).await;
            let _ = stderr_task.await;
            return Err(PythonSupervisorError::cancelled_with(termination));
        }
        _ = tokio::time::sleep_until(deadline) => {
            drop(job);
            let _ = child.wait().await;
            let _ = stderr_task.await;
            return Err(PythonSupervisorError::timeout());
        }
    };

    let _stderr = stderr_task
        .await
        .map_err(|_| PythonSupervisorError::failed("stderr task failed"))??;

    if !status.success() {
        return Err(PythonSupervisorError::failed("child failed"));
    }

    drop(job);

    Ok(count)
}

#[cfg(windows)]
pub async fn run_python_scan<F>(
    app_root: &Path,
    roots: &[PathBuf],
    timeout: std::time::Duration,
    cancellation: tokio_util::sync::CancellationToken,
    on_discovery: F,
) -> Result<u64, PythonSupervisorError>
where
    F: FnMut(Discovery),
{
    let spec = python_launch_spec(app_root)
        .map_err(|_| PythonSupervisorError::failed("staged Python unavailable"))?;

    let roots: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();

    let request = encode_scan_request("python.config.scan", &roots)
        .map_err(|_| PythonSupervisorError::protocol())?;

    let args: Vec<std::ffi::OsString> = spec.args.iter().map(std::ffi::OsString::from).collect();

    run_supervised_process(
        &spec.program,
        &args,
        &spec.current_dir,
        &spec.removed_env,
        &request,
        SupervisorControl {
            timeout,
            cancellation,
        },
        on_discovery,
    )
    .await
}

#[derive(Debug, Deserialize)]
struct PythonResponse {
    protocol_version: u64,
    kind: String,
    discovery: Option<PythonDiscovery>,
    count: Option<u64>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PythonDiscovery {
    fingerprint: String,
    suggested_name: String,
    suggested_type: String,
    confidence: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PythonScannerEvent {
    Discovery(Discovery),
    Completed {
        count: u64,
    },
    Pong,
    Cancelled,
    Error {
        code: String,
        message: Option<String>,
    },
}

pub fn parse_python_response(line: &str) -> Result<PythonScannerEvent, &'static str> {
    let response: PythonResponse = serde_json::from_str(line).map_err(|_| SCANNER_PROTOCOL)?;

    if response.protocol_version != 1 {
        return Err(SCANNER_PROTOCOL);
    }

    match response.kind.as_str() {
        "discovery" => translate_discovery_response(line).map(PythonScannerEvent::Discovery),
        "pong" => Ok(PythonScannerEvent::Pong),
        "cancelled" => Ok(PythonScannerEvent::Cancelled),
        "error" => Ok(PythonScannerEvent::Error {
            code: response.code.ok_or(SCANNER_PROTOCOL)?,
            message: response.message,
        }),
        "completed" => Ok(PythonScannerEvent::Completed {
            count: response.count.ok_or(SCANNER_PROTOCOL)?,
        }),
        _ => Err(SCANNER_PROTOCOL),
    }
}
fn translate_discovery_response(line: &str) -> Result<Discovery, &'static str> {
    let response: PythonResponse = serde_json::from_str(line).map_err(|_| SCANNER_PROTOCOL)?;

    if response.protocol_version != 1 || response.kind != "discovery" {
        return Err(SCANNER_PROTOCOL);
    }

    let python = response.discovery.ok_or(SCANNER_PROTOCOL)?;

    let suggested_type = match python.suggested_type.as_str() {
        "mcp" => ToolKind::Mcp,
        "claude" | "codex" | "docker" | "ollama" => ToolKind::Configuration,
        "unknown" => ToolKind::Unknown,
        _ => return Err(SCANNER_PROTOCOL),
    };

    let confidence = match python.confidence.as_str() {
        "low" => Confidence::Low,
        "medium" => Confidence::Medium,
        "high" => Confidence::High,
        _ => return Err(SCANNER_PROTOCOL),
    };

    let mut discovery =
        Discovery::unknown(python.suggested_name, "python.config", python.fingerprint);
    discovery.suggested_type = suggested_type;
    discovery.confidence = confidence;
    discovery.evidence = python.evidence;

    Ok(discovery)
}

#[cfg(test)]
mod tests {
    use crate::{Confidence, ObservedState, ToolKind};

    use super::{PythonScannerEvent, parse_python_response, translate_discovery_response};

    #[test]
    fn translates_mcp_discovery_into_rust_domain_model() {
        let line = r#"{
            "protocol_version":1,"request_id":"req-1",
            "kind":"discovery",
            "discovery":{
                "fingerprint":"abc123",
                "suggested_name":"config",
                "suggested_type":"mcp",
                "confidence":"medium",
                "evidence":[
                    {"kind":"path","summary":"C:\\Users\\test\\.mcp\\config.json"},
                    {"kind":"reason","summary":"configuration contains an MCP server mapping"}
                ],
                "health_state":"unknown"
            }
        }"#;

        let discovery = translate_discovery_response(line).unwrap();

        assert_eq!(discovery.fingerprint, "abc123");
        assert_eq!(discovery.suggested_name, "config");
        assert_eq!(discovery.suggested_type, ToolKind::Mcp);
        assert_eq!(discovery.source_scanner, "python.config");
        assert_eq!(discovery.confidence, Confidence::Medium);
        assert_eq!(discovery.evidence.len(), 2);

        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.registration_state, ObservedState::Unknown);
        assert_eq!(discovery.enablement_state, ObservedState::Unknown);
        assert_eq!(discovery.runtime_state, ObservedState::Unknown);
        assert_eq!(discovery.connection_state, ObservedState::Unknown);
        assert_eq!(discovery.authentication_state, ObservedState::Unknown);
        assert_eq!(discovery.health_state, ObservedState::Unknown);
    }

    #[test]
    fn rejects_unrecognized_python_tool_kind() {
        let line = r#"{
            "request_id":"req-2",
            "kind":"discovery",
            "discovery":{
                "fingerprint":"def456",
                "suggested_name":"mystery",
                "suggested_type":"invented_kind",
                "confidence":"low",
                "evidence":[],
                "health_state":"unknown"
            }
        }"#;

        assert_eq!(translate_discovery_response(line), Err("scanner_protocol"));
    }

    #[test]
    fn maps_remaining_python_product_labels() {
        for (label, expected) in [
            ("claude", ToolKind::Configuration),
            ("codex", ToolKind::Configuration),
            ("docker", ToolKind::Configuration),
            ("ollama", ToolKind::Configuration),
            ("unknown", ToolKind::Unknown),
        ] {
            let line = format!(
                r#"{{"protocol_version":1,"kind":"discovery","discovery":{{"fingerprint":"abc","suggested_name":"config","suggested_type":"{label}","confidence":"low","evidence":[]}}}}"#
            );
            let discovery = translate_discovery_response(&line).unwrap();
            assert_eq!(discovery.suggested_type, expected);
        }
    }

    #[test]
    fn rejects_wrong_python_protocol_version() {
        let line = r#"{"protocol_version":2,"kind":"discovery","discovery":{"fingerprint":"abc","suggested_name":"config","suggested_type":"mcp","confidence":"low","evidence":[]}}"#;

        assert_eq!(translate_discovery_response(line), Err("scanner_protocol"));
    }

    #[test]
    fn parses_completed_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-3","kind":"completed","count":4}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Completed { count: 4 })
        );
    }

    #[test]
    fn parses_pong_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-ping","kind":"pong"}"#;

        assert_eq!(parse_python_response(line), Ok(PythonScannerEvent::Pong));
    }

    #[test]
    fn parses_cancelled_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-cancel","kind":"cancelled"}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Cancelled)
        );
    }

    #[test]
    fn parses_error_response() {
        let line = r#"{"protocol_version":1,"kind":"error","code":"scanner_failed","message":"safe message"}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Error {
                code: "scanner_failed".to_string(),
                message: Some("safe message".to_string()),
            })
        );
    }

    #[test]
    fn rejects_malformed_python_json() {
        assert_eq!(parse_python_response("{not-json"), Err("scanner_protocol"));
    }
}

#[cfg(test)]
mod bounded_io_tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn rejects_stdout_line_larger_than_one_mibibyte() {
        let (mut writer, reader) = tokio::io::duplex(8 * 1024);

        let writer_task = tokio::spawn(async move {
            let payload = vec![b'x'; MAX_MESSAGE_BYTES + 1];
            writer.write_all(&payload).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        });

        let mut reader = BufReader::new(reader);
        let mut total_stdout = 0usize;

        assert_eq!(
            read_bounded_line(&mut reader, &mut total_stdout).await,
            Err(SCANNER_PROTOCOL)
        );

        writer_task.abort();
    }
}

#[cfg(all(test, windows))]
mod job_object_tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::process::Command;

    #[tokio::test]
    async fn closing_job_object_terminates_attached_child() {
        let mut child = Command::new("ping.exe")
            .args(["127.0.0.1", "-n", "30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let job = WindowsJob::new().unwrap();
        job.assign(&child).unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "child exited before job close"
        );

        drop(job);

        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("child survived job close")
            .unwrap();
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn streams_discoveries_until_completed() {
        let (mut writer, reader) = tokio::io::duplex(4096);

        let writer_task = tokio::spawn(async move {
            writer
                .write_all(
                    br#"{"protocol_version":1,"kind":"discovery","discovery":{"fingerprint":"abc","suggested_name":"config","suggested_type":"mcp","confidence":"medium","evidence":[]}}
{"protocol_version":1,"kind":"completed","count":1}
"#,
                )
                .await
                .unwrap();
        });

        let mut reader = BufReader::new(reader);
        let mut names = Vec::new();

        let count = consume_python_stdout(&mut reader, |discovery| {
            names.push(discovery.suggested_name);
        })
        .await
        .unwrap();

        writer_task.await.unwrap();

        assert_eq!(names, vec!["config"]);
        assert_eq!(count, 1);
    }
}

#[cfg(test)]
mod stderr_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn stderr_is_bounded_drained_and_redacted() {
        let (mut writer, reader) = tokio::io::duplex(4096);

        let writer_task = tokio::spawn(async move {
            writer
                .write_all(b"Authorization: Bearer top-secret\n")
                .await
                .unwrap();
            writer.write_all(&vec![b'x'; 300 * 1024]).await.unwrap();
        });

        let captured = capture_stderr(reader).await.unwrap();

        writer_task.await.unwrap();

        assert!(captured.len() <= 256 * 1024);
        assert!(!captured.contains("top-secret"));
        assert!(captured.contains("[REDACTED]"));
    }
}

#[cfg(all(test, windows))]
mod supervised_process_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn supervised_process_writes_one_request_and_reads_completed() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-helper.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine('{"protocol_version":1,"kind":"completed","count":0}')
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-1","operation":"scan","roots":[]}
"#;

        let count = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(5),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(count, 0);
    }
}

#[cfg(all(test, windows))]
mod timeout_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn supervised_process_returns_scanner_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-timeout.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
Start-Sleep -Seconds 30
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-timeout","operation":"scan","roots":[]}
"#;

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_millis(200),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_timeout");
    }

    #[tokio::test]
    async fn completed_child_that_stays_alive_still_times_out() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-completed-then-sleeps.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine('{"protocol_version":1,"kind":"completed","count":0}')
Start-Sleep -Seconds 3
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-completed-timeout","operation":"scan","roots":[]}
"#;

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_millis(1500),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_timeout");
    }
}

/// Serializes the handful of tests below that spawn a real `powershell.exe`
/// (which itself spawns a `ping.exe` descendant) and assert on tight, fixed
/// wall-clock windows around that external process's startup, cancellation,
/// and teardown. Under CPU contention (e.g. several of these launching
/// PowerShell at once, or a busy CI runner), PowerShell interpreter startup
/// can outlast those fixed windows and produce a spurious failure — not a
/// production bug, a test-fixture race. Reproduced locally by running this
/// module's test set under artificial concurrent load (multiple `cargo test`
/// invocations racing for CPU) before adding this lock.
///
/// Held for a whole test's body so no two of these process-heavy tests ever
/// launch/verify their external process concurrently within one test binary.
/// Scoped to just these tests (not `--test-threads=1` for the whole crate)
/// per the smallest-fix principle: every other test in this crate remains
/// fully parallel.
#[cfg(all(test, windows))]
static WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[cfg(all(test, windows))]
mod cancellation_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn supervised_process_returns_scanner_cancelled() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-cancel.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
Start-Sleep -Seconds 30
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-cancel","operation":"scan","roots":[]}
"#;

        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            trigger.cancel();
        });

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_millis(600),
                cancellation,
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_cancelled");
    }

    #[tokio::test]
    async fn completed_child_that_stays_alive_can_still_be_cancelled() {
        let _serialize = super::WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-completed-then-cancel.ps1");
        let ready_file = temp.path().join("completed-ready.txt");

        std::fs::write(
            &script,
            r#"param([string]$ReadyPath)
$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine('{"protocol_version":1,"kind":"completed","count":0}')
Set-Content -LiteralPath $ReadyPath -Value "ready"
Start-Sleep -Seconds 5
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
            ready_file.clone().into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-completed-cancel","operation":"scan","roots":[]}
"#;

        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        let ready = ready_file.clone();

        tokio::spawn(async move {
            for _ in 0..100 {
                if ready.is_file() {
                    trigger.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(2),
                cancellation,
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_cancelled");
    }
}

#[tokio::test]
async fn rejects_stdout_after_total_64_mibibyte_limit() {
    use tokio::io::BufReader;

    let reader = tokio::io::empty();
    let mut reader = BufReader::new(reader);
    let mut total_stdout = MAX_STDOUT_BYTES + 1;

    assert_eq!(
        read_bounded_line(&mut reader, &mut total_stdout).await,
        Ok(None)
    );

    let (mut writer, reader) = tokio::io::duplex(64);
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        writer.write_all(b"{}\n").await.unwrap();
    });

    let mut reader = BufReader::new(reader);
    let mut total_stdout = MAX_STDOUT_BYTES;

    assert_eq!(
        read_bounded_line(&mut reader, &mut total_stdout).await,
        Err(SCANNER_PROTOCOL)
    );
}

#[cfg(all(test, windows))]
mod malformed_protocol_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn malformed_child_json_returns_scanner_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-malformed.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine('{not-json')
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-malformed","operation":"scan","roots":[]}
"#;

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(5),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_protocol");
    }
}

#[cfg(all(test, windows))]
mod oversized_protocol_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn oversized_child_line_returns_scanner_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-oversized.ps1");

        std::fs::write(
            &script,
            r#"$null = [Console]::In.ReadLine()
[Console]::Out.WriteLine(('x' * 1048577))
"#,
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-oversized","operation":"scan","roots":[]}
"#;

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(5),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_protocol");
    }
}

#[cfg(all(test, windows))]
mod descendant_cleanup_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn timeout_kills_descendant_processes() {
        let _serialize = super::WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-descendant.ps1");
        let pid_file = temp.path().join("descendant.pid");

        std::fs::write(
            &script,
            format!(
                r#"$null = [Console]::In.ReadLine()
$child = Start-Process ping.exe -ArgumentList '127.0.0.1','-n','30' -PassThru
Set-Content -Path '{}' -Value $child.Id
Start-Sleep -Seconds 30
"#,
                pid_file.display()
            ),
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-descendant","operation":"scan","roots":[]}
"#;

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_millis(1500),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_timeout");

        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let alive = std::process::Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();

        let output = String::from_utf8_lossy(&alive.stdout);
        assert!(
            !output.contains(&pid.to_string()),
            "descendant process {pid} survived supervisor timeout"
        );
    }
}

#[cfg(all(test, windows))]
mod cancellation_descendant_cleanup_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn cancellation_kills_descendant_processes_after_grace_period() {
        let _serialize = super::WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-cancel-descendant.ps1");
        let pid_file = temp.path().join("cancel-descendant.pid");

        std::fs::write(
            &script,
            format!(
                r#"$null = [Console]::In.ReadLine()
$child = Start-Process ping.exe -ArgumentList '127.0.0.1','-n','30' -PassThru
Set-Content -Path '{}' -Value $child.Id
Start-Sleep -Seconds 30
"#,
                pid_file.display()
            ),
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-cancel-descendant","operation":"scan","roots":[]}
"#;

        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();

        let pid_wait = pid_file.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                if pid_wait.is_file() {
                    trigger.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        });

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(10),
                cancellation,
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_cancelled");

        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let output = std::process::Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();

        let output = String::from_utf8_lossy(&output.stdout);
        assert!(
            !output.contains(&pid.to_string()),
            "descendant process {pid} survived supervisor cancellation"
        );
    }

    #[tokio::test]
    async fn cancellation_past_grace_period_reports_forced_termination() {
        let _serialize = super::WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-cancel-forced-descendant.ps1");
        let pid_file = temp.path().join("cancel-forced-descendant.pid");

        // This child ignores stdin shutdown: it never reads stdin again after
        // the initial ReadLine, and keeps its descendant alive well past the
        // 2-second grace period so the supervisor must fall back to closing
        // the Windows Job Object.
        std::fs::write(
            &script,
            format!(
                r#"$null = [Console]::In.ReadLine()
$child = Start-Process ping.exe -ArgumentList '127.0.0.1','-n','30' -PassThru
Set-Content -Path '{}' -Value $child.Id
Start-Sleep -Seconds 30
"#,
                pid_file.display()
            ),
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-cancel-forced-descendant","operation":"scan","roots":[]}
"#;

        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();

        let pid_wait = pid_file.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                if pid_wait.is_file() {
                    trigger.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        });

        let error = run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(10),
                cancellation,
            },
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "scanner_cancelled");
        assert_eq!(error.termination(), Some(PythonTermination::Forced));

        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let output = std::process::Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();

        let output = String::from_utf8_lossy(&output.stdout);
        assert!(
            !output.contains(&pid.to_string()),
            "descendant process {pid} survived forced supervisor cancellation"
        );
    }
}

#[cfg(all(test, windows))]
mod drop_cleanup_tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn dropping_supervisor_kills_descendant_processes() {
        let _serialize = super::WINDOWS_PROCESS_SUPERVISOR_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("scanner-drop-descendant.ps1");
        let pid_file = temp.path().join("drop-descendant.pid");

        std::fs::write(
            &script,
            format!(
                r#"$null = [Console]::In.ReadLine()
$child = Start-Process ping.exe -ArgumentList '127.0.0.1','-n','30' -PassThru
Set-Content -Path '{}' -Value $child.Id
Start-Sleep -Seconds 30
"#,
                pid_file.display()
            ),
        )
        .unwrap();

        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-File"),
            script.into_os_string(),
        ];

        let request = r#"{"protocol_version":1,"request_id":"req-drop-descendant","operation":"scan","roots":[]}
"#;

        let mut supervisor = Box::pin(run_supervised_process(
            &powershell,
            &args,
            temp.path(),
            &[],
            request,
            SupervisorControl {
                timeout: Duration::from_secs(30),
                cancellation: CancellationToken::new(),
            },
            |_| {},
        ));

        let wait_for_pid = async {
            for _ in 0..120 {
                if let Ok(contents) = std::fs::read_to_string(&pid_file)
                    && contents.trim().parse::<u32>().is_ok()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            panic!("descendant PID file was never readable");
        };

        tokio::select! {
            result = &mut supervisor => panic!("supervisor exited early: {result:?}"),
            _ = wait_for_pid => {}
        }

        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        drop(supervisor);

        tokio::time::sleep(Duration::from_millis(150)).await;

        let output = std::process::Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();

        let output = String::from_utf8_lossy(&output.stdout);
        assert!(
            !output.contains(&pid.to_string()),
            "descendant process {pid} survived supervisor drop"
        );
    }
}
