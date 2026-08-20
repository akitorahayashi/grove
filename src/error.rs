use std::error::Error;
use std::fmt;
use std::io;

/// Stable top-level categories for failures returned by grove's public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppErrorKind {
    Io,
    Configuration,
    Cache,
    InvalidArguments,
    InvalidRepositoryName,
    RepositoryNotFound,
    Git,
    Zoxide,
    Internal,
}

/// A public API failure with an evolution-safe category and typed source chain.
#[derive(Debug)]
pub struct AppError {
    detail: Detail,
}

#[derive(Debug)]
enum Detail {
    Io(io::Error),
    Configuration(ConfigError),
    Cache(CacheError),
    InvalidArguments(ArgumentError),
    InvalidRepositoryName(String),
    RepositoryNotFound(String),
    Git(GitError),
    Zoxide(ZoxideError),
    Internal(InternalError),
}

impl AppError {
    pub fn kind(&self) -> AppErrorKind {
        match &self.detail {
            Detail::Io(_) => AppErrorKind::Io,
            Detail::Configuration(_) => AppErrorKind::Configuration,
            Detail::Cache(_) => AppErrorKind::Cache,
            Detail::InvalidArguments(_) => AppErrorKind::InvalidArguments,
            Detail::InvalidRepositoryName(_) => AppErrorKind::InvalidRepositoryName,
            Detail::RepositoryNotFound(_) => AppErrorKind::RepositoryNotFound,
            Detail::Git(_) => AppErrorKind::Git,
            Detail::Zoxide(_) => AppErrorKind::Zoxide,
            Detail::Internal(_) => AppErrorKind::Internal,
        }
    }

    pub fn io_error(&self) -> Option<&io::Error> {
        match &self.detail {
            Detail::Io(error) => Some(error),
            _ => None,
        }
    }

    pub fn configuration_error(&self) -> Option<&ConfigError> {
        match &self.detail {
            Detail::Configuration(error) => Some(error),
            _ => None,
        }
    }

    pub fn cache_error(&self) -> Option<&CacheError> {
        match &self.detail {
            Detail::Cache(error) => Some(error),
            _ => None,
        }
    }

    pub fn argument_error(&self) -> Option<&ArgumentError> {
        match &self.detail {
            Detail::InvalidArguments(error) => Some(error),
            _ => None,
        }
    }

    pub fn git_error(&self) -> Option<&GitError> {
        match &self.detail {
            Detail::Git(error) => Some(error),
            _ => None,
        }
    }

    pub fn zoxide_error(&self) -> Option<&ZoxideError> {
        match &self.detail {
            Detail::Zoxide(error) => Some(error),
            _ => None,
        }
    }

    pub fn repository_name(&self) -> Option<&str> {
        match &self.detail {
            Detail::InvalidRepositoryName(name) | Detail::RepositoryNotFound(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn config_error(message: impl Into<String>) -> Self {
        Self { detail: Detail::Configuration(ConfigError::new(message)) }
    }

    pub(crate) fn config_source(
        message: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self { detail: Detail::Configuration(ConfigError::with_source(message, source)) }
    }

    pub(crate) fn cache_state(message: impl Into<String>) -> Self {
        Self { detail: Detail::Cache(CacheError::new(message)) }
    }

    pub(crate) fn invalid_arguments(message: impl Into<String>) -> Self {
        Self { detail: Detail::InvalidArguments(ArgumentError::new(message)) }
    }

    pub(crate) fn invalid_repository_name(name: impl Into<String>) -> Self {
        Self { detail: Detail::InvalidRepositoryName(name.into()) }
    }

    pub(crate) fn repository_not_found(name: impl Into<String>) -> Self {
        Self { detail: Detail::RepositoryNotFound(name.into()) }
    }

    pub(crate) fn git_unavailable(message: impl Into<String>) -> Self {
        Self { detail: Detail::Git(GitError::unavailable(message)) }
    }

    pub(crate) fn git_unavailable_status(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self { detail: Detail::Git(GitError::unavailable_status(command, message, exit_code)) }
    }

    pub(crate) fn git_unavailable_source(source: io::Error) -> Self {
        let message = source.to_string();
        Self { detail: Detail::Git(GitError::unavailable_source(message, source)) }
    }

    pub(crate) fn git_command_failed(
        command: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self { detail: Detail::Git(GitError::command_failed(command, message, None, None)) }
    }

    pub(crate) fn git_command_failed_status(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self { detail: Detail::Git(GitError::command_failed(command, message, exit_code, None)) }
    }

    pub(crate) fn git_command_failed_source(command: impl Into<String>, source: io::Error) -> Self {
        let message = source.to_string();
        Self { detail: Detail::Git(GitError::command_failed(command, message, None, Some(source))) }
    }

    pub(crate) fn zoxide_unavailable_status(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            detail: Detail::Zoxide(ZoxideError::unavailable_status(command, message, exit_code)),
        }
    }

    pub(crate) fn zoxide_unavailable_source(source: io::Error) -> Self {
        let message = source.to_string();
        Self { detail: Detail::Zoxide(ZoxideError::unavailable_source(message, source)) }
    }

    #[cfg(test)]
    pub(crate) fn zoxide_command_failed(
        command: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self { detail: Detail::Zoxide(ZoxideError::command_failed(command, message, None, None)) }
    }

    pub(crate) fn zoxide_command_failed_status(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            detail: Detail::Zoxide(ZoxideError::command_failed(command, message, exit_code, None)),
        }
    }

    pub(crate) fn zoxide_command_failed_source(
        command: impl Into<String>,
        source: io::Error,
    ) -> Self {
        let message = source.to_string();
        Self {
            detail: Detail::Zoxide(ZoxideError::command_failed(
                command,
                message,
                None,
                Some(source),
            )),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self { detail: Detail::Internal(InternalError::new(message)) }
    }

    pub(crate) fn is_internal(&self) -> bool {
        self.kind() == AppErrorKind::Internal
    }

    /// Demote a per-repository boundary failure to its message for the report.
    /// A genuine internal error keeps propagating and aborts the run.
    pub(crate) fn demote(self) -> Result<String, AppError> {
        if self.is_internal() { Err(self) } else { Ok(self.to_string()) }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Detail::Io(error) => error.fmt(formatter),
            Detail::Configuration(error) => error.fmt(formatter),
            Detail::Cache(error) => error.fmt(formatter),
            Detail::InvalidArguments(error) => error.fmt(formatter),
            Detail::InvalidRepositoryName(name) => {
                write!(formatter, "invalid repository name: {name}")
            }
            Detail::RepositoryNotFound(name) => {
                write!(formatter, "repository '{name}' was not found in grove.toml")
            }
            Detail::Git(error) => error.fmt(formatter),
            Detail::Zoxide(error) => error.fmt(formatter),
            Detail::Internal(error) => write!(formatter, "internal application failure: {error}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.detail {
            Detail::Io(error) => Some(error),
            Detail::Configuration(error) => Some(error),
            Detail::Cache(error) => Some(error),
            Detail::InvalidArguments(error) => Some(error),
            Detail::Git(error) => Some(error),
            Detail::Zoxide(error) => Some(error),
            Detail::Internal(error) => Some(error),
            Detail::InvalidRepositoryName(_) | Detail::RepositoryNotFound(_) => None,
        }
    }
}

impl From<io::Error> for AppError {
    fn from(error: io::Error) -> Self {
        Self { detail: Detail::Io(error) }
    }
}

macro_rules! message_error {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name {
            message: String,
        }

        impl $name {
            fn new(message: impl Into<String>) -> Self {
                Self { message: message.into() }
            }

            pub fn message(&self) -> &str {
                &self.message
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.message)
            }
        }

        impl Error for $name {}
    };
}

message_error!(CacheError);
message_error!(ArgumentError);
message_error!(InternalError);

#[derive(Debug)]
pub struct ConfigError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), source: None }
    }

    fn with_source(message: impl Into<String>, source: impl Error + Send + Sync + 'static) -> Self {
        Self { message: message.into(), source: Some(Box::new(source)) }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as &(dyn Error + 'static))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProcessErrorKind {
    Unavailable,
    CommandFailed,
}

/// The storage every external tool's process failure shares. `GitError` and
/// `ZoxideError` remain separate public types so a caller can match on which
/// tool failed, but they differ only in the label their `Display` prints, which
/// each wrapper supplies to `render`.
#[derive(Debug)]
struct ProcessError {
    kind: ProcessErrorKind,
    command: Option<String>,
    message: String,
    exit_code: Option<i32>,
    source: Option<io::Error>,
}

impl ProcessError {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: ProcessErrorKind::Unavailable,
            command: None,
            message: bounded_diagnostic(message),
            exit_code: None,
            source: None,
        }
    }

    fn unavailable_source(message: impl Into<String>, source: io::Error) -> Self {
        Self { source: Some(source), ..Self::unavailable(message) }
    }

    fn unavailable_status(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self { command: Some(command.into()), exit_code, ..Self::unavailable(message) }
    }

    fn command_failed(
        command: impl Into<String>,
        message: impl Into<String>,
        exit_code: Option<i32>,
        source: Option<io::Error>,
    ) -> Self {
        Self {
            kind: ProcessErrorKind::CommandFailed,
            command: Some(command.into()),
            message: bounded_diagnostic(message),
            exit_code,
            source,
        }
    }

    fn render(&self, formatter: &mut fmt::Formatter<'_>, tool: &str) -> fmt::Result {
        match (&self.kind, &self.command) {
            (ProcessErrorKind::Unavailable, _) => {
                write!(formatter, "{tool} is not available: {}", self.message)
            }
            (ProcessErrorKind::CommandFailed, Some(command)) => {
                write!(formatter, "{tool} command failed: {command}: {}", self.message)
            }
            (ProcessErrorKind::CommandFailed, None) => {
                write!(formatter, "{tool} command failed: {}", self.message)
            }
        }
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|source| source as &(dyn Error + 'static))
    }
}

macro_rules! process_error {
    ($name:ident, $tool:literal) => {
        #[derive(Debug)]
        pub struct $name(ProcessError);

        impl $name {
            fn unavailable_source(message: impl Into<String>, source: io::Error) -> Self {
                Self(ProcessError::unavailable_source(message, source))
            }

            fn unavailable_status(
                command: impl Into<String>,
                message: impl Into<String>,
                exit_code: Option<i32>,
            ) -> Self {
                Self(ProcessError::unavailable_status(command, message, exit_code))
            }

            fn command_failed(
                command: impl Into<String>,
                message: impl Into<String>,
                exit_code: Option<i32>,
                source: Option<io::Error>,
            ) -> Self {
                Self(ProcessError::command_failed(command, message, exit_code, source))
            }

            pub fn kind(&self) -> ProcessErrorKind {
                self.0.kind
            }

            pub fn command(&self) -> Option<&str> {
                self.0.command.as_deref()
            }

            pub fn diagnostic(&self) -> &str {
                &self.0.message
            }

            pub fn exit_code(&self) -> Option<i32> {
                self.0.exit_code
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.render(formatter, $tool)
            }
        }

        impl Error for $name {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                self.0.source()
            }
        }
    };
}

process_error!(GitError, "git");
process_error!(ZoxideError, "zoxide");

impl GitError {
    fn unavailable(message: impl Into<String>) -> Self {
        Self(ProcessError::unavailable(message))
    }
}

const MAX_PROCESS_DIAGNOSTIC_BYTES: usize = 64 * 1024 + 2048;

fn bounded_diagnostic(message: impl Into<String>) -> String {
    let message = message.into();
    if message.len() <= MAX_PROCESS_DIAGNOSTIC_BYTES {
        return message;
    }

    let mut start = message.len() - MAX_PROCESS_DIAGNOSTIC_BYTES;
    while !message.is_char_boundary(start) {
        start += 1;
    }
    format!("[process diagnostic output truncated]\n{}", &message[start..])
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::{AppError, AppErrorKind, ProcessErrorKind};

    #[test]
    fn io_errors_remain_available_through_the_source_chain() {
        let error = AppError::from(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));

        assert_eq!(error.kind(), AppErrorKind::Io);
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<io::Error>())
                .map(io::Error::kind),
            Some(io::ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn process_exit_errors_expose_command_status_and_bounded_diagnostics() {
        let diagnostic = "x".repeat(super::MAX_PROCESS_DIAGNOSTIC_BYTES + 1);
        let error = AppError::git_command_failed_status("git fetch origin", diagnostic, Some(42));
        let process = error.git_error().expect("Git detail should exist");

        assert_eq!(process.kind(), ProcessErrorKind::CommandFailed);
        assert_eq!(process.command(), Some("git fetch origin"));
        assert_eq!(process.exit_code(), Some(42));
        assert!(process.diagnostic().starts_with("[process diagnostic output truncated]\n"));
        assert!(process.diagnostic().len() <= super::MAX_PROCESS_DIAGNOSTIC_BYTES + 40);
    }

    #[test]
    fn process_spawn_errors_remain_available_through_the_source_chain() {
        let error = AppError::git_command_failed_source(
            "git status",
            io::Error::new(io::ErrorKind::NotFound, "git missing"),
        );
        let process = error.git_error().expect("Git detail should exist");

        assert_eq!(
            process
                .source()
                .and_then(|source| source.downcast_ref::<io::Error>())
                .map(io::Error::kind),
            Some(io::ErrorKind::NotFound)
        );
    }
}
