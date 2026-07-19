use std::io;

/// Library-wide error type for grove.
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    ConfigError(String),

    #[error("invalid repository name: {0}")]
    InvalidRepositoryName(String),

    #[error("repository '{0}' was not found in grove.toml")]
    RepositoryNotFound(String),

    #[error("git is not available: {0}")]
    GitUnavailable(String),

    #[error("git command failed: {command}: {message}")]
    GitCommandFailed { command: String, message: String },
}

impl AppError {
    pub fn config_error<S: Into<String>>(message: S) -> Self {
        AppError::ConfigError(message.into())
    }

    pub fn git_command_failed<C: Into<String>, M: Into<String>>(command: C, message: M) -> Self {
        AppError::GitCommandFailed { command: command.into(), message: message.into() }
    }

    /// Provide an `io::ErrorKind`-like view for callers that need coarse error handling.
    pub fn kind(&self) -> io::ErrorKind {
        match self {
            Self::Io(err) => err.kind(),
            Self::Json(_)
            | Self::ConfigError(_)
            | Self::InvalidRepositoryName(_)
            | Self::GitUnavailable(_)
            | Self::GitCommandFailed { .. } => io::ErrorKind::InvalidInput,
            Self::RepositoryNotFound(_) => io::ErrorKind::NotFound,
        }
    }
}
