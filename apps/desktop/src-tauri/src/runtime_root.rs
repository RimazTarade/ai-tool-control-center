use control_center_core::PythonRootError;
use std::path::{Path, PathBuf};

fn absolute_existing_dir(path: &Path) -> Result<PathBuf, PythonRootError> {
    let canonical = path.canonicalize().map_err(|_| PythonRootError)?;
    if canonical.is_absolute() {
        Ok(canonical)
    } else {
        Err(PythonRootError)
    }
}

#[cfg(any(test, debug_assertions))]
fn development_app_root(manifest_dir: &Path) -> Result<PathBuf, PythonRootError> {
    absolute_existing_dir(&manifest_dir.join("..").join("..").join(".."))
}

#[cfg(any(test, not(debug_assertions)))]
fn packaged_app_root(executable: &Path) -> Result<PathBuf, PythonRootError> {
    if !executable.is_absolute() {
        return Err(PythonRootError);
    }

    let parent = executable.parent().ok_or(PythonRootError)?;
    absolute_existing_dir(parent)
}

pub(crate) fn resolve_python_app_root() -> Result<PathBuf, PythonRootError> {
    #[cfg(debug_assertions)]
    {
        development_app_root(Path::new(env!("CARGO_MANIFEST_DIR")))
    }

    #[cfg(not(debug_assertions))]
    {
        let executable = std::env::current_exe().map_err(|_| PythonRootError)?;
        packaged_app_root(&executable)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ai-tool-control-runtime-root-test-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn development_root_is_repository_root_and_absolute() {
        let root = temporary_root();
        let manifest_dir = root.join("apps").join("desktop").join("src-tauri");
        fs::create_dir_all(&manifest_dir).unwrap();

        let resolved = development_app_root(&manifest_dir).unwrap();
        let expected = root.canonicalize().unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, expected);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packaged_root_is_executable_parent_and_absolute() {
        let root = temporary_root();
        let executable = root.join("AI Tool Control Center.exe");

        let resolved = packaged_app_root(&executable).unwrap();
        let expected = root.canonicalize().unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, expected);

        fs::remove_dir_all(root).unwrap();
    }
}
