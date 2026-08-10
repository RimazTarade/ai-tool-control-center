use crate::{Confidence, Discovery, Evidence, ObservedState, ToolKind};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// Returns a stable identity key for Windows paths.
///
/// Windows path identity is case-insensitive for the discovery purposes used
/// here. Both slash styles are normalized so observations from PATH, registry,
/// configuration files, and native APIs can be compared consistently.
pub fn windows_path_key(path: &Path) -> String {
    let mut key = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();

    while key.ends_with('\\') && !is_drive_root(&key) {
        key.pop();
    }

    key
}

/// Removes duplicate Windows paths while preserving the first observation.
pub fn dedupe_windows_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for path in paths {
        if seen.insert(windows_path_key(&path)) {
            deduped.push(path);
        }
    }

    deduped
}

/// Parses a semicolon-separated Windows PATH-style value into unique entries.
///
/// Surrounding whitespace is ignored, a single pair of surrounding quotes is
/// removed, and empty entries are discarded before Windows-style
/// case-insensitive deduplication.
pub fn parse_windows_path_entries(raw: &str) -> Vec<PathBuf> {
    let entries = raw.split(';').filter_map(|entry| {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return None;
        }

        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(trimmed);

        Some(PathBuf::from(unquoted.trim()))
    });

    dedupe_windows_paths(entries)
}

/// Observes executable files directly present in Windows PATH directories.
///
/// This is an observation scanner only. It does not execute discovered files
/// and every result remains pending for the mandatory review queue.
pub fn discover_path_executables(path_value: &str, pathext_value: &str) -> Vec<Discovery> {
    let executable_extensions: HashSet<String> = pathext_value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('.').to_ascii_lowercase())
        .collect();

    if executable_extensions.is_empty() {
        return Vec::new();
    }

    let mut discoveries = Vec::new();
    for directory in parse_windows_path_entries(path_value) {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() || !has_executable_extension(&path, &executable_extensions) {
                continue;
            }

            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };

            let mut discovery = Discovery::unknown(
                name,
                "windows.path",
                fingerprint_windows_path(&path),
            );
            discovery.suggested_type = ToolKind::Cli;
            discovery.confidence = Confidence::Medium;
            discovery.evidence.push(Evidence {
                kind: "path".into(),
                summary: path.display().to_string(),
            });
            discoveries.push(discovery);
        }
    }

    discoveries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryView {
    Registry32,
    Registry64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallRegistryRecord {
    pub hive: RegistryHive,
    pub view: RegistryView,
    pub key_name: String,
    pub display_name: Option<String>,
    pub install_location: Option<PathBuf>,
    pub publisher: Option<String>,
}

pub trait UninstallRegistrySource {
    fn read_uninstall_entries(
        &self,
        hive: RegistryHive,
        view: RegistryView,
    ) -> Result<Vec<UninstallRegistryRecord>, String>;
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsUninstallRegistrySource;

#[cfg(windows)]
impl UninstallRegistrySource for WindowsUninstallRegistrySource {
    fn read_uninstall_entries(
        &self,
        hive: RegistryHive,
        view: RegistryView,
    ) -> Result<Vec<UninstallRegistryRecord>, String> {
        use std::io::ErrorKind;
        use winreg::{
            enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
            HKCU, HKLM,
        };

        const UNINSTALL_PATH: &str =
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

        let root = match hive {
            RegistryHive::CurrentUser => &HKCU,
            RegistryHive::LocalMachine => &HKLM,
        };
        let view_flag = match view {
            RegistryView::Registry32 => KEY_WOW64_32KEY,
            RegistryView::Registry64 => KEY_WOW64_64KEY,
        };
        let permissions = KEY_READ | view_flag;

        let uninstall = match root.open_subkey_with_flags(UNINSTALL_PATH, permissions) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "failed to open {hive:?} {view:?} uninstall key: {error}"
                ));
            }
        };

        let mut records = Vec::new();
        for key_name in uninstall.enum_keys().filter_map(Result::ok) {
            let Ok(app_key) = uninstall.open_subkey_with_flags(&key_name, permissions) else {
                continue;
            };

            let display_name = app_key.get_value::<String, _>("DisplayName").ok();
            let install_location = app_key
                .get_value::<String, _>("InstallLocation")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            let publisher = app_key
                .get_value::<String, _>("Publisher")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());

            records.push(UninstallRegistryRecord {
                hive,
                view,
                key_name,
                display_name,
                install_location,
                publisher,
            });
        }

        Ok(records)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallRegistryError {
    pub hive: RegistryHive,
    pub view: RegistryView,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct UninstallRegistryReport {
    pub discoveries: Vec<Discovery>,
    pub errors: Vec<UninstallRegistryError>,
}

/// Converts uninstall-registry observations into reviewable discoveries.
///
/// All four user/machine and 32/64-bit views are attempted independently.
/// Failure to read one view is reported without discarding successful results
/// from the remaining views.
pub fn discover_uninstall_registry_with(
    source: &impl UninstallRegistrySource,
) -> UninstallRegistryReport {
    const LOCATIONS: [(RegistryHive, RegistryView); 4] = [
        (RegistryHive::CurrentUser, RegistryView::Registry64),
        (RegistryHive::CurrentUser, RegistryView::Registry32),
        (RegistryHive::LocalMachine, RegistryView::Registry64),
        (RegistryHive::LocalMachine, RegistryView::Registry32),
    ];

    let mut report = UninstallRegistryReport::default();

    for (hive, view) in LOCATIONS {
        let records = match source.read_uninstall_entries(hive, view) {
            Ok(records) => records,
            Err(message) => {
                report.errors.push(UninstallRegistryError {
                    hive,
                    view,
                    message,
                });
                continue;
            }
        };

        for record in records {
            let Some(name) = record
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };

            let mut discovery = Discovery::unknown(
                name,
                "windows.uninstall_registry",
                fingerprint_registry_record(&record),
            );
            discovery.suggested_type = ToolKind::DesktopApplication;
            discovery.confidence = Confidence::High;
            discovery.evidence.push(Evidence {
                kind: "registry".into(),
                summary: registry_evidence_summary(&record, name),
            });
            report.discoveries.push(discovery);
        }
    }

    report
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub name: String,
    pub executable_path: Option<PathBuf>,
}

pub trait ProcessSource {
    fn read_processes(&self) -> Result<Vec<ProcessRecord>, String>;
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsProcessSource;

#[cfg(windows)]
impl ProcessSource for WindowsProcessSource {
    fn read_processes(&self) -> Result<Vec<ProcessRecord>, String> {
        use std::{io, mem::size_of};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        };

        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "failed to create Windows process snapshot: {}",
                io::Error::last_os_error()
            ));
        }

        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            let _ = unsafe { CloseHandle(snapshot) };
            return Err(format!(
                "failed to read first Windows process snapshot entry: {error}"
            ));
        }

        let mut records = Vec::new();
        loop {
            let pid = entry.th32ProcessID;
            if pid != 0 {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);

                if !name.trim().is_empty() {
                    records.push(ProcessRecord {
                        pid,
                        name,
                        executable_path: windows_process_executable_path(pid),
                    });
                }
            }

            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };
        Ok(records)
    }
}

/// Converts running process observations into pending discoveries.
///
/// Executable paths provide the strongest stable identity. When a path is not
/// available, the normalized process name is used instead.
pub fn discover_processes_with(source: &impl ProcessSource) -> Result<Vec<Discovery>, String> {
    let records = source.read_processes()?;
    let mut seen = HashSet::new();
    let mut discoveries = Vec::new();

    for record in records {
        let raw_name = record.name.trim();
        if raw_name.is_empty() {
            continue;
        }

        let name = Path::new(raw_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(raw_name);

        let identity = record
            .executable_path
            .as_ref()
            .map(|path| format!("path:{}", windows_path_key(path)))
            .unwrap_or_else(|| format!("name:{}", raw_name.to_ascii_lowercase()));

        if !seen.insert(identity.clone()) {
            continue;
        }

        let mut discovery = Discovery::unknown(
            name,
            "windows.process",
            fingerprint_process_identity(&identity),
        );
        discovery.runtime_state = ObservedState::Running;
        discovery.confidence = if record.executable_path.is_some() {
            Confidence::High
        } else {
            Confidence::Medium
        };
        discovery.evidence.push(Evidence {
            kind: "process".into(),
            summary: process_evidence_summary(&record),
        });
        discoveries.push(discovery);
    }

    Ok(discoveries)
}

#[cfg(windows)]
fn windows_process_executable_path(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    };

    const PROCESS_PATH_CAPACITY: usize = 32_768;

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }

    let mut buffer = vec![0u16; PROCESS_PATH_CAPACITY];
    let mut length = buffer.len() as u32;
    let success =
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    let _ = unsafe { CloseHandle(process) };

    if success == 0 || length == 0 {
        return None;
    }

    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceRuntimeState {
    Running,
    Stopped,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceRecord {
    pub service_name: String,
    pub display_name: Option<String>,
    pub runtime_state: ServiceRuntimeState,
}

pub trait ServiceSource {
    fn read_services(&self) -> Result<Vec<ServiceRecord>, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TcpEndpointRecord {
    pub local_address: String,
    pub local_port: u16,
    pub owning_pid: Option<u32>,
}

pub trait TcpEndpointSource {
    fn read_listening_endpoints(&self) -> Result<Vec<TcpEndpointRecord>, String>;
}

/// Converts listening TCP endpoint observations into pending discoveries.
pub fn discover_tcp_endpoints_with(
    source: &impl TcpEndpointSource,
) -> Result<Vec<Discovery>, String> {
    let records = source.read_listening_endpoints()?;
    let mut seen = HashSet::new();
    let mut discoveries = Vec::new();

    for record in records {
        let address = record.local_address.trim();
        if address.is_empty() || record.local_port == 0 {
            continue;
        }

        let normalized_address = address.to_ascii_lowercase();
        let identity = format!("{normalized_address}:{}", record.local_port);
        if !seen.insert(identity.clone()) {
            continue;
        }

        let suggested_name = if address.contains(':') {
            format!("[{address}]:{}", record.local_port)
        } else {
            format!("{address}:{}", record.local_port)
        };

        let fingerprint: String = Sha256::digest(format!("tcp:{identity}").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

        let mut discovery =
            Discovery::unknown(suggested_name, "windows.tcp", fingerprint);
        discovery.suggested_type = ToolKind::LocalService;
        discovery.confidence = Confidence::Medium;
        discovery.runtime_state = ObservedState::Running;
        discovery.evidence.push(Evidence {
            kind: "tcp".into(),
            summary: format!(
                "address={address} port={} pid={}",
                record.local_port,
                record
                    .owning_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ),
        });
        discoveries.push(discovery);
    }

    Ok(discoveries)
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsServiceSource;

#[cfg(windows)]
impl ServiceSource for WindowsServiceSource {
    fn read_services(&self) -> Result<Vec<ServiceRecord>, String> {
        use std::{io, mem::size_of, ptr, slice};
        use windows_sys::Win32::{
            Foundation::ERROR_MORE_DATA,
            System::Services::{
                CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW,
                OpenSCManagerW, SC_ENUM_PROCESS_INFO, SC_MANAGER_ENUMERATE_SERVICE,
                SERVICE_RUNNING, SERVICE_STATE_ALL, SERVICE_STOPPED, SERVICE_WIN32,
            },
        };

        const ENUM_BUFFER_BYTES: usize = 256 * 1024;
        const MAX_ENUMERATION_CHUNKS: usize = 64;

        let manager =
            unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_ENUMERATE_SERVICE) };
        if manager.is_null() {
            return Err(format!(
                "failed to open Windows service control manager: {}",
                io::Error::last_os_error()
            ));
        }

        let result = (|| {
            let words = ENUM_BUFFER_BYTES.div_ceil(size_of::<usize>());
            let mut resume_handle = 0u32;
            let mut records = Vec::new();

            for _ in 0..MAX_ENUMERATION_CHUNKS {
                // Use usize storage so the Win32 structures in the byte buffer
                // have pointer-width alignment on both 32-bit and 64-bit Windows.
                let mut buffer = vec![0usize; words];
                let mut bytes_needed = 0u32;
                let mut services_returned = 0u32;

                let success = unsafe {
                    EnumServicesStatusExW(
                        manager,
                        SC_ENUM_PROCESS_INFO,
                        SERVICE_WIN32,
                        SERVICE_STATE_ALL,
                        buffer.as_mut_ptr().cast::<u8>(),
                        ENUM_BUFFER_BYTES as u32,
                        &mut bytes_needed,
                        &mut services_returned,
                        &mut resume_handle,
                        ptr::null(),
                    )
                };

                if services_returned > 0 {
                    let entries = unsafe {
                        slice::from_raw_parts(
                            buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                            services_returned as usize,
                        )
                    };

                    for entry in entries {
                        let Some(service_name) =
                            (unsafe { windows_service_wide_string(entry.lpServiceName) })
                        else {
                            continue;
                        };
                        if service_name.trim().is_empty() {
                            continue;
                        }

                        let display_name =
                            unsafe { windows_service_wide_string(entry.lpDisplayName) }
                                .map(|value| value.trim().to_string())
                                .filter(|value| !value.is_empty());

                        let runtime_state = match entry.ServiceStatusProcess.dwCurrentState {
                            SERVICE_RUNNING => ServiceRuntimeState::Running,
                            SERVICE_STOPPED => ServiceRuntimeState::Stopped,
                            _ => ServiceRuntimeState::Unknown,
                        };

                        records.push(ServiceRecord {
                            service_name,
                            display_name,
                            runtime_state,
                        });
                    }
                }

                if success != 0 {
                    return Ok(records);
                }

                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                    return Err(format!("failed to enumerate Windows services: {error}"));
                }

                if services_returned == 0 {
                    return Err(format!(
                        "Windows service enumeration made no progress ({} bytes still needed)",
                        bytes_needed
                    ));
                }
            }

            Err(format!(
                "Windows service enumeration exceeded {MAX_ENUMERATION_CHUNKS} chunks"
            ))
        })();

        let _ = unsafe { CloseServiceHandle(manager) };
        result
    }
}

#[cfg(windows)]
unsafe fn windows_service_wide_string(value: *const u16) -> Option<String> {
    const MAX_SERVICE_STRING_UNITS: usize = 1024;

    if value.is_null() {
        return None;
    }

    let mut length = 0usize;
    while length < MAX_SERVICE_STRING_UNITS {
        if unsafe { *value.add(length) } == 0 {
            let units = unsafe { std::slice::from_raw_parts(value, length) };
            return Some(String::from_utf16_lossy(units));
        }
        length += 1;
    }

    None
}

/// Converts Windows service observations into pending discoveries.
pub fn discover_services_with(source: &impl ServiceSource) -> Result<Vec<Discovery>, String> {
    let records = source.read_services()?;
    let mut seen = HashSet::new();
    let mut discoveries = Vec::new();

    for record in records {
        let service_name = record.service_name.trim();
        if service_name.is_empty() {
            continue;
        }

        let identity = service_name.to_ascii_lowercase();
        if !seen.insert(identity.clone()) {
            continue;
        }

        let suggested_name = record
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(service_name);

        let mut discovery = Discovery::unknown(
            suggested_name,
            "windows.service",
            fingerprint_service_identity(&identity),
        );
        discovery.suggested_type = ToolKind::WindowsService;
        discovery.confidence = Confidence::High;
        discovery.registration_state = ObservedState::Registered;
        discovery.runtime_state = match record.runtime_state {
            ServiceRuntimeState::Running => ObservedState::Running,
            ServiceRuntimeState::Stopped => ObservedState::Stopped,
            ServiceRuntimeState::Unknown => ObservedState::Unknown,
        };
        discovery.evidence.push(Evidence {
            kind: "service".into(),
            summary: service_evidence_summary(&record),
        });
        discoveries.push(discovery);
    }

    Ok(discoveries)
}

fn fingerprint_service_identity(identity: &str) -> String {
    Sha256::digest(format!("service:{identity}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn service_evidence_summary(record: &ServiceRecord) -> String {
    let state = match record.runtime_state {
        ServiceRuntimeState::Running => "running",
        ServiceRuntimeState::Stopped => "stopped",
        ServiceRuntimeState::Unknown => "unknown",
    };
    let mut parts = vec![
        format!("service_name={}", record.service_name.trim()),
        format!("state={state}"),
    ];

    if let Some(display_name) = record
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        parts.push(format!("display_name={display_name}"));
    }

    parts.join("; ")
}

fn has_executable_extension(path: &Path, extensions: &HashSet<String>) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| extensions.contains(&extension))
}

fn fingerprint_windows_path(path: &Path) -> String {
    Sha256::digest(windows_path_key(path).as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fingerprint_process_identity(identity: &str) -> String {
    Sha256::digest(identity.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn process_evidence_summary(record: &ProcessRecord) -> String {
    let mut parts = vec![
        format!("pid={}", record.pid),
        format!("name={}", record.name.trim()),
    ];

    if let Some(path) = record.executable_path.as_ref() {
        parts.push(format!("executable={}", path.display()));
    }

    parts.join("; ")
}

fn fingerprint_registry_record(record: &UninstallRegistryRecord) -> String {
    let identity = format!(
        "{}|{}|{}",
        registry_hive_key(record.hive),
        registry_view_key(record.view),
        record.key_name.trim().to_ascii_lowercase()
    );

    Sha256::digest(identity.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn registry_evidence_summary(record: &UninstallRegistryRecord, name: &str) -> String {
    let mut parts = vec![
        format!("name={name}"),
        format!("hive={}", registry_hive_key(record.hive)),
        format!("view={}", registry_view_key(record.view)),
        format!("key={}", record.key_name),
    ];

    if let Some(location) = record.install_location.as_ref() {
        parts.push(format!("install_location={}", location.display()));
    }

    if let Some(publisher) = record
        .publisher
        .as_deref()
        .map(str::trim)
        .filter(|publisher| !publisher.is_empty())
    {
        parts.push(format!("publisher={publisher}"));
    }

    parts.join("; ")
}

fn registry_hive_key(hive: RegistryHive) -> &'static str {
    match hive {
        RegistryHive::CurrentUser => "hkcu",
        RegistryHive::LocalMachine => "hklm",
    }
}

fn registry_view_key(view: RegistryView) -> &'static str {
    match view {
        RegistryView::Registry32 => "32",
        RegistryView::Registry64 => "64",
    }
}

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\'
}
