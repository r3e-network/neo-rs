use alloc::string::String;

use neo_base::Bytes;
use thiserror::Error;

use crate::manifest::PermissionKind;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("method {method} not found")]
    MethodNotFound { method: String },

    #[error("invalid parameters")]
    InvalidParameters,

    #[error("insufficient permissions: required {0:?}")]
    PermissionDenied(PermissionKind),

    #[error("native contract {0} not registered")]
    NativeNotFound(String),

    #[error("runtime error: {0}")]
    Runtime(&'static str),

    #[error("serialization error")]
    Serialization,

    #[error("storage error: {0}")]
    Storage(&'static str),

    #[error("script returned fault: {0:?}")]
    Fault(Bytes),
}
