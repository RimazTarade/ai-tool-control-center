//! Deep Scan filesystem safety policy for Windows.
//!
//! These helpers classify filesystem roots and directory entries *before* any
//! content read is attempted, so Task 5's Deep Scan traversal engine can:
//!
//! - require explicit confirmation before crossing onto a network root
//!   (UNC path or mapped network drive), via [`classify_root`];
//! - skip cloud-only / offline / recall-on-access placeholder files without
//!   ever opening their content, via [`entry_policy`];
//! - detect symlink/junction cycles when reparse-point following is enabled,
//!   via [`stable_directory_identity`].
//!
//! Symbolic links and junctions are never followed unless a caller opts in
//! per-scan; this module only classifies, it never decides to follow.

use std::io;
use std::path::Path;

/// Whether a filesystem root is local storage or reachable only over the
/// network (UNC path or mapped network drive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootLocation {
    Local,
    Network,
}

/// Safety-relevant attributes of a filesystem entry, checked before any
/// content operation is attempted on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EntryPolicy {
    /// The entry is a reparse point (symlink, junction, or similar). Deep
    /// Scan does not follow these by default.
    pub reparse_point: bool,
    /// The entry is a cloud-only, offline, or recall-on-access placeholder.
    /// Milestone 6 never hydrates placeholder content.
    pub placeholder: bool,
}

/// A stable identity for a directory, used to detect cycles when following
/// reparse points is explicitly enabled.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DirectoryIdentity(String);

#[cfg(test)]
impl DirectoryIdentity {
    /// Test-only constructor. Task 5's fake filesystem needs to hand out
    /// deterministic, non-filesystem-backed identities to simulate cycles
    /// and hard-linked directories without touching a real disk; the field
    /// stays private everywhere else so production code can only obtain an
    /// identity via [`stable_directory_identity`].
    pub(crate) fn for_test(id: impl Into<String>) -> Self {
        DirectoryIdentity(id.into())
    }
}

#[cfg(windows)]
pub(crate) fn classify_root(path: &Path) -> io::Result<RootLocation> {
    use std::path::{Component, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };
    use windows::core::PCWSTR;

    let prefix = match path.components().next() {
        Some(Component::Prefix(prefix_component)) => prefix_component.kind(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "path has no drive or UNC prefix to classify: {}",
                    path.display()
                ),
            ));
        }
    };

    match prefix {
        Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) => Ok(RootLocation::Network),
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            let drive_root = format!("{}:\\", letter as char);
            let wide: Vec<u16> = drive_root
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
            match drive_type {
                DRIVE_REMOTE => Ok(RootLocation::Network),
                DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK | DRIVE_CDROM => {
                    Ok(RootLocation::Local)
                }
                other => Err(io::Error::other(format!(
                    "GetDriveTypeW returned an unclassifiable drive type ({other}) for {drive_root}"
                ))),
            }
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unsupported path prefix for root classification: {}",
                path.display()
            ),
        )),
    }
}

#[cfg(not(windows))]
pub(crate) fn classify_root(path: &Path) -> io::Result<RootLocation> {
    if path.to_string_lossy().starts_with("//") {
        Ok(RootLocation::Network)
    } else {
        Ok(RootLocation::Local)
    }
}

#[cfg(windows)]
pub(crate) fn entry_policy(path: &Path) -> io::Result<EntryPolicy> {
    use std::os::windows::fs::MetadataExt;

    // symlink_metadata never follows a reparse point, so this reflects the
    // entry itself rather than whatever it may point at.
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(classify_attributes(metadata.file_attributes()))
}

#[cfg(not(windows))]
pub(crate) fn entry_policy(path: &Path) -> io::Result<EntryPolicy> {
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(EntryPolicy {
        reparse_point: metadata.file_type().is_symlink(),
        placeholder: false,
    })
}

#[cfg(windows)]
fn classify_attributes(attributes: u32) -> EntryPolicy {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
        FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let placeholder = attributes & FILE_ATTRIBUTE_OFFLINE.0 != 0
        || attributes & FILE_ATTRIBUTE_RECALL_ON_OPEN.0 != 0
        || attributes & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0 != 0;
    let reparse_point = attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;

    EntryPolicy {
        reparse_point,
        placeholder,
    }
}

#[cfg(windows)]
pub(crate) fn stable_directory_identity(path: &Path) -> io::Result<DirectoryIdentity> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    // Deliberately no FILE_FLAG_OPEN_REPARSE_POINT: when a caller follows a
    // reparse point, cycle detection needs the identity of the directory the
    // link resolves to, not the link itself.
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(|error| {
        io::Error::other(format!(
            "failed to open directory {}: {error}",
            path.display()
        ))
    })?;

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(handle, &mut info) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|error| {
        io::Error::other(format!(
            "failed to read directory identity for {}: {error}",
            path.display()
        ))
    })?;

    Ok(DirectoryIdentity(format!(
        "{:08x}-{:08x}{:08x}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    )))
}

#[cfg(not(windows))]
pub(crate) fn stable_directory_identity(path: &Path) -> io::Result<DirectoryIdentity> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(DirectoryIdentity(canonical.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
        FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    #[cfg(windows)]
    #[test]
    fn offline_and_recall_attributes_are_placeholders() {
        assert!(classify_attributes(FILE_ATTRIBUTE_OFFLINE.0).placeholder);
        assert!(classify_attributes(FILE_ATTRIBUTE_RECALL_ON_OPEN.0).placeholder);
        assert!(classify_attributes(FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0).placeholder);
    }

    #[cfg(windows)]
    #[test]
    fn reparse_attribute_is_reported_separately() {
        let policy = classify_attributes(FILE_ATTRIBUTE_REPARSE_POINT.0);
        assert!(policy.reparse_point);
        assert!(!policy.placeholder);
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_attributes_are_neither_placeholder_nor_reparse_point() {
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;

        let policy = classify_attributes(FILE_ATTRIBUTE_NORMAL.0);
        assert!(!policy.placeholder);
        assert!(!policy.reparse_point);
    }

    #[cfg(windows)]
    #[test]
    fn unc_path_is_classified_as_network_without_touching_the_network() {
        let path = Path::new(r"\\server\share\folder");
        assert_eq!(classify_root(path).unwrap(), RootLocation::Network);
    }

    #[test]
    fn stable_directory_identity_is_stable_across_calls() {
        let temp = tempfile::tempdir().expect("create temp dir");

        let first = stable_directory_identity(temp.path()).unwrap();
        let second = stable_directory_identity(temp.path()).unwrap();

        assert_eq!(first, second);
    }
}
