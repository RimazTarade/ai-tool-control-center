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

const KNOWN_LOCATION_EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "target",
    "Cache",
    "Caches",
    ".cache",
    "Code Cache",
    "GPUCache",
    "Temp",
    "tmp",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownLocationKind {
    Programs,
    Launcher,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownLocationRoot {
    pub kind: KnownLocationKind,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownLocationError {
    pub root: PathBuf,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct KnownLocationReport {
    pub discoveries: Vec<Discovery>,
    pub errors: Vec<KnownLocationError>,
}

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsKnownLocationRootReport {
    pub roots: Vec<KnownLocationRoot>,
    pub errors: Vec<String>,
}

#[cfg(windows)]
pub fn windows_known_location_roots() -> WindowsKnownLocationRootReport {
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
        FOLDERID_Programs,
    };

    let locations = [
        (
            &FOLDERID_ProgramFiles,
            KnownLocationKind::Programs,
            "Program Files",
        ),
        (
            &FOLDERID_ProgramFilesX86,
            KnownLocationKind::Programs,
            "Program Files (x86)",
        ),
        (
            &FOLDERID_Programs,
            KnownLocationKind::Launcher,
            "user Start Menu programs",
        ),
        (
            &FOLDERID_CommonPrograms,
            KnownLocationKind::Launcher,
            "common Start Menu programs",
        ),
    ];

    let mut report = WindowsKnownLocationRootReport::default();
    let mut seen = HashSet::new();

    for (folder_id, kind, label) in locations {
        match windows_known_folder_path(folder_id) {
            Ok(path) if path.is_dir() => {
                if seen.insert(windows_path_key(&path)) {
                    report.roots.push(KnownLocationRoot { kind, path });
                }
            }
            Ok(path) => report.errors.push(format!(
                "{label}: known-folder path is not an existing directory: {}",
                path.display()
            )),
            Err(message) => report.errors.push(format!("{label}: {message}")),
        }
    }

    report
}

#[cfg(windows)]
fn windows_known_folder_path(folder_id: &windows_sys::core::GUID) -> Result<PathBuf, String> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::SHGetKnownFolderPath,
    };

    let mut raw_path = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(folder_id, 0, std::ptr::null_mut(), &mut raw_path) };

    if result < 0 {
        return Err(format!(
            "SHGetKnownFolderPath failed with HRESULT 0x{:08X}",
            result as u32
        ));
    }
    if raw_path.is_null() {
        return Err("SHGetKnownFolderPath returned a null path".into());
    }

    let mut len = 0usize;
    while unsafe { *raw_path.add(len) } != 0 {
        len += 1;
        if len >= 32_768 {
            unsafe { CoTaskMemFree(raw_path.cast::<core::ffi::c_void>()) };
            return Err("known-folder path exceeded the Windows UTF-16 path bound".into());
        }
    }

    let path = PathBuf::from(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(raw_path, len)
    }));
    unsafe { CoTaskMemFree(raw_path.cast::<core::ffi::c_void>()) };

    Ok(path)
}

/// Observes executable and launcher files under explicitly supplied Windows roots.
///
/// Traversal is depth-bounded, does not follow links, skips common high-noise
/// directories, and reports a root failure without discarding discoveries from
/// other roots.
pub fn discover_known_locations(
    roots: &[KnownLocationRoot],
    max_depth: usize,
) -> KnownLocationReport {
    let mut report = KnownLocationReport::default();
    let mut seen = HashSet::new();

    for root in roots {
        if !root.path.exists() {
            continue;
        }

        if !root.path.is_dir() {
            report.errors.push(KnownLocationError {
                root: root.path.clone(),
                message: "known location root is not a directory".into(),
            });
            continue;
        }

        let entries = walkdir::WalkDir::new(&root.path)
            .follow_links(false)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !KNOWN_LOCATION_EXCLUDED.iter().any(|excluded| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(excluded)
                    })
            });

        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(error) => {
                    report.errors.push(KnownLocationError {
                        root: root.path.clone(),
                        message: format!("failed to read known location: {error}"),
                    });
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(suggested_type) = known_location_tool_kind(path) else {
                continue;
            };
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };

            let identity = windows_path_key(path);
            if !seen.insert(identity) {
                continue;
            }

            let mut discovery = Discovery::unknown(
                name,
                "windows.known_location",
                fingerprint_windows_path(path),
            );
            discovery.suggested_type = suggested_type;
            discovery.confidence = Confidence::Medium;
            discovery.evidence.push(Evidence {
                kind: "path".into(),
                summary: format!(
                    "location_kind={}; path={}",
                    known_location_kind_key(root.kind),
                    path.display()
                ),
            });
            report.discoveries.push(discovery);
        }
    }

    report
}

fn known_location_tool_kind(path: &Path) -> Option<ToolKind> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("exe") => Some(ToolKind::DesktopApplication),
        Some("cmd" | "bat" | "ps1") => Some(ToolKind::Cli),
        Some("lnk") => Some(ToolKind::Launcher),
        _ => None,
    }
}

fn known_location_kind_key(kind: KnownLocationKind) -> &'static str {
    match kind {
        KnownLocationKind::Programs => "programs",
        KnownLocationKind::Launcher => "launcher",
    }
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

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsTcpEndpointSource;

#[cfg(windows)]
impl TcpEndpointSource for WindowsTcpEndpointSource {
    fn read_listening_endpoints(&self) -> Result<Vec<TcpEndpointRecord>, String> {
        use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

        let mut records = windows_tcp_ipv4_listeners(AF_INET as u32)?;
        records.extend(windows_tcp_ipv6_listeners(AF_INET6 as u32)?);
        Ok(records)
    }
}

#[cfg(windows)]
fn windows_tcp_table_buffer(address_family: u32) -> Result<Vec<usize>, String> {
    use std::{io, mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER,
        NetworkManagement::IpHelper::{GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER},
    };

    const MAX_BUFFER_ATTEMPTS: usize = 4;

    let mut required_bytes = 0u32;
    let first_status = unsafe {
        GetExtendedTcpTable(
            ptr::null_mut(),
            &mut required_bytes,
            0,
            address_family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };

    if first_status != ERROR_INSUFFICIENT_BUFFER && first_status != 0 {
        return Err(format!(
            "failed to size Windows TCP listener table for address family {address_family}: {}",
            io::Error::from_raw_os_error(first_status as i32)
        ));
    }

    if required_bytes == 0 {
        return Ok(Vec::new());
    }

    for _ in 0..MAX_BUFFER_ATTEMPTS {
        let words = (required_bytes as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0usize; words];
        let mut buffer_bytes = (buffer.len() * size_of::<usize>()) as u32;

        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut buffer_bytes,
                0,
                address_family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };

        if status == 0 {
            return Ok(buffer);
        }

        if status != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!(
                "failed to enumerate Windows TCP listeners for address family {address_family}: {}",
                io::Error::from_raw_os_error(status as i32)
            ));
        }

        if buffer_bytes <= required_bytes {
            return Err(format!(
                "Windows TCP listener table for address family {address_family} requested a larger buffer without increasing its size"
            ));
        }

        required_bytes = buffer_bytes;
    }

    Err(format!(
        "Windows TCP listener table for address family {address_family} changed size too many times"
    ))
}

#[cfg(windows)]
fn windows_tcp_ipv4_listeners(address_family: u32) -> Result<Vec<TcpEndpointRecord>, String> {
    use std::{
        mem::{offset_of, size_of},
        net::Ipv4Addr,
        slice,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    };

    let buffer = windows_tcp_table_buffer(address_family)?;
    if buffer.is_empty() {
        return Ok(Vec::new());
    }

    let buffer_bytes = buffer.len() * size_of::<usize>();
    if buffer_bytes < size_of::<u32>() {
        return Err("Windows IPv4 TCP listener table was shorter than its entry count".into());
    }

    let table = buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
    let entry_count = unsafe { (*table).dwNumEntries as usize };
    let rows_offset = offset_of!(MIB_TCPTABLE_OWNER_PID, table);

    if buffer_bytes < rows_offset {
        return Err("Windows IPv4 TCP listener table had an invalid row offset".into());
    }

    let available_rows = (buffer_bytes - rows_offset) / size_of::<MIB_TCPROW_OWNER_PID>();
    if entry_count > available_rows {
        return Err(format!(
            "Windows IPv4 TCP listener table reported {entry_count} rows but only {available_rows} fit in the returned buffer"
        ));
    }

    let rows = unsafe {
        let rows_ptr = buffer
            .as_ptr()
            .cast::<u8>()
            .add(rows_offset)
            .cast::<MIB_TCPROW_OWNER_PID>();
        slice::from_raw_parts(rows_ptr, entry_count)
    };

    Ok(rows
        .iter()
        .map(|row| TcpEndpointRecord {
            local_address: Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string(),
            local_port: u16::from_be(row.dwLocalPort as u16),
            owning_pid: (row.dwOwningPid != 0).then_some(row.dwOwningPid),
        })
        .collect())
}

#[cfg(windows)]
fn windows_tcp_ipv6_listeners(address_family: u32) -> Result<Vec<TcpEndpointRecord>, String> {
    use std::{
        mem::{offset_of, size_of},
        net::Ipv6Addr,
        slice,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
    };

    let buffer = windows_tcp_table_buffer(address_family)?;
    if buffer.is_empty() {
        return Ok(Vec::new());
    }

    let buffer_bytes = buffer.len() * size_of::<usize>();
    if buffer_bytes < size_of::<u32>() {
        return Err("Windows IPv6 TCP listener table was shorter than its entry count".into());
    }

    let table = buffer.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
    let entry_count = unsafe { (*table).dwNumEntries as usize };
    let rows_offset = offset_of!(MIB_TCP6TABLE_OWNER_PID, table);

    if buffer_bytes < rows_offset {
        return Err("Windows IPv6 TCP listener table had an invalid row offset".into());
    }

    let available_rows = (buffer_bytes - rows_offset) / size_of::<MIB_TCP6ROW_OWNER_PID>();
    if entry_count > available_rows {
        return Err(format!(
            "Windows IPv6 TCP listener table reported {entry_count} rows but only {available_rows} fit in the returned buffer"
        ));
    }

    let rows = unsafe {
        let rows_ptr = buffer
            .as_ptr()
            .cast::<u8>()
            .add(rows_offset)
            .cast::<MIB_TCP6ROW_OWNER_PID>();
        slice::from_raw_parts(rows_ptr, entry_count)
    };

    Ok(rows
        .iter()
        .map(|row| {
            let address = Ipv6Addr::from(row.ucLocalAddr);
            let local_address = if row.dwLocalScopeId == 0 {
                address.to_string()
            } else {
                format!("{address}%{}", row.dwLocalScopeId)
            };

            TcpEndpointRecord {
                local_address,
                local_port: u16::from_be(row.dwLocalPort as u16),
                owning_pid: (row.dwOwningPid != 0).then_some(row.dwOwningPid),
            }
        })
        .collect())
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
