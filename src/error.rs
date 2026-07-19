use std::io;

/// Library-wide error type capturing domain-neutral and underlying I/O failures.
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] io::Error),

    /// Configuration or environment issue that prevents command execution.
    #[error("{0}")]
    ConfigError(String),

    /// Raised when a requested item cannot be located in storage.
    #[error("Item '{0}' was not found")]
    ItemNotFound(String),

    /// Raised when a requested label cannot be located in storage.
    #[error("Label '{0}' was not found")]
    LabelNotFound(String),

    /// Raised when an item identifier fails validation.
    #[error("invalid item identifier: {0}")]
    InvalidItemId(String),

    /// Raised when a label name fails validation.
    #[error("invalid label name: {0}")]
    InvalidLabelName(String),

    /// Raised when detaching a label that is not attached to an item.
    #[error("Label '{label_name}' is not attached to item '{item_id}'")]
    LabelingNotFound { item_id: String, label_name: String },
}

impl AppError {
    pub fn config_error<S: Into<String>>(message: S) -> Self {
        AppError::ConfigError(message.into())
    }

    /// Provide an `io::ErrorKind`-like view for callers expecting legacy behavior.
    pub fn kind(&self) -> io::ErrorKind {
        match self {
            AppError::Io(err) => err.kind(),
            AppError::ConfigError(_)
            | AppError::InvalidItemId(_)
            | AppError::InvalidLabelName(_) => io::ErrorKind::InvalidInput,
            AppError::ItemNotFound(_)
            | AppError::LabelNotFound(_)
            | AppError::LabelingNotFound { .. } => io::ErrorKind::NotFound,
        }
    }
}
