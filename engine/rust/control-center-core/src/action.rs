use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionSource {
    BuiltIn,
    UserAuthored,
    CommunityAdapter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandSpec {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout_seconds: u64,
    pub requires_administrator: bool,
    pub source: ActionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionPreview {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout_seconds: u64,
    pub requires_administrator: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("executable must be an absolute path")]
    RelativeExecutable,
    #[error("working directory must be an absolute path")]
    RelativeWorkingDirectory,
    #[error("community adapters cannot define executable actions")]
    CommunityExecutable,
    #[error("shell interpreters are not accepted by the structured action boundary")]
    ShellInterpreter,
    #[error("administrator actions are disabled in unsigned builds")]
    UnsignedAdministrator,
    #[error("timeout must be between 1 and 300 seconds")]
    InvalidTimeout,
    #[error("an argument contains a NUL character")]
    InvalidArgument,
}

impl CommandSpec {
    pub fn preview(&self, signed_build: bool) -> Result<ActionPreview, ActionError> {
        if !self.executable.is_absolute() {
            return Err(ActionError::RelativeExecutable);
        }
        if !self.working_directory.is_absolute() {
            return Err(ActionError::RelativeWorkingDirectory);
        }
        if self.source == ActionSource::CommunityAdapter {
            return Err(ActionError::CommunityExecutable);
        }
        if is_shell(&self.executable) {
            return Err(ActionError::ShellInterpreter);
        }
        if self.requires_administrator && !signed_build {
            return Err(ActionError::UnsignedAdministrator);
        }
        if !(1..=300).contains(&self.timeout_seconds) {
            return Err(ActionError::InvalidTimeout);
        }
        if self
            .arguments
            .iter()
            .any(|argument| argument.contains('\0'))
        {
            return Err(ActionError::InvalidArgument);
        }
        Ok(ActionPreview {
            executable: self.executable.clone(),
            arguments: self.arguments.clone(),
            working_directory: self.working_directory.clone(),
            timeout_seconds: self.timeout_seconds,
            requires_administrator: self.requires_administrator,
        })
    }
}

fn is_shell(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "cmd" | "powershell" | "pwsh"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> CommandSpec {
        CommandSpec {
            executable: PathBuf::from(r"C:\Program Files\Example\example.exe"),
            arguments: vec!["--status".into(), "value & still-an-argument".into()],
            working_directory: PathBuf::from(r"C:\Program Files\Example"),
            timeout_seconds: 10,
            requires_administrator: false,
            source: ActionSource::BuiltIn,
        }
    }

    #[test]
    fn permits_metacharacters_as_direct_arguments_but_rejects_shells() {
        assert!(command().preview(false).is_ok());
        let mut shell = command();
        shell.executable = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        assert_eq!(shell.preview(false), Err(ActionError::ShellInterpreter));
    }

    #[test]
    fn rejects_community_and_unsigned_admin_commands() {
        let mut community = command();
        community.source = ActionSource::CommunityAdapter;
        assert_eq!(
            community.preview(false),
            Err(ActionError::CommunityExecutable)
        );

        let mut elevated = command();
        elevated.requires_administrator = true;
        assert_eq!(
            elevated.preview(false),
            Err(ActionError::UnsignedAdministrator)
        );
    }
}
