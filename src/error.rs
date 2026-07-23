use std::io;

/// Library-wide error type for grove.
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("{0}")]
    ConfigError(String),

    #[error("{0}")]
    CacheState(String),

    #[error("{0}")]
    InvalidArguments(String),

    #[error("invalid repository name: {0}")]
    InvalidRepositoryName(String),

    #[error("repository '{0}' was not found in grove.toml")]
    RepositoryNotFound(String),

    #[error("git is not available: {0}")]
    GitUnavailable(String),

    #[error("git command failed: {command}: {message}")]
    GitCommandFailed { command: String, message: String },

    #[error("zoxide is not available: {0}")]
    ZoxideUnavailable(String),

    #[error("zoxide command failed: {command}: {message}")]
    ZoxideCommandFailed { command: String, message: String },

    #[error("internal application failure: {0}")]
    Internal(String),
}

impl AppError {
    pub(crate) fn config_error<S: Into<String>>(message: S) -> Self {
        AppError::ConfigError(message.into())
    }

    pub(crate) fn cache_state<S: Into<String>>(message: S) -> Self {
        AppError::CacheState(message.into())
    }

    pub(crate) fn invalid_arguments<S: Into<String>>(message: S) -> Self {
        AppError::InvalidArguments(message.into())
    }

    pub(crate) fn git_command_failed<C: Into<String>, M: Into<String>>(
        command: C,
        message: M,
    ) -> Self {
        AppError::GitCommandFailed { command: command.into(), message: message.into() }
    }

    pub(crate) fn zoxide_command_failed<C: Into<String>, M: Into<String>>(
        command: C,
        message: M,
    ) -> Self {
        AppError::ZoxideCommandFailed { command: command.into(), message: message.into() }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Whether this error is an internal application failure that must abort a
    /// run rather than be demoted to a per-repository outcome. Owning this
    /// decision here keeps the demotion policy from being re-encoded at each
    /// use-case call site.
    pub(crate) fn is_internal(&self) -> bool {
        matches!(self, AppError::Internal(_))
    }
}
