use thiserror::Error;
use crate::uint160::ParseUInt160Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("Invalid manifest format: {0}")]
    InvalidFormat(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for field {0}: {1}")]
    InvalidFieldValue(String, String),

    #[error("Duplicate entry: {0}")]
    DuplicateEntry(String),

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("ParseUInt160Error: {0}")]
    ParseUInt160Error(#[from] ParseUInt160Error),

    InvalidStackItemType,
}
