// Copyright (C) 2015-2025 The Neo Project.
//
// Shared token error type mirroring the exceptions thrown by the C# REST plugin.

use crate::rest_server::helpers::script_helper::ScriptHelperError;
use neo_core::UInt160;
use thiserror::Error;

/// Errors surfaced while interacting with NEP-17/NEP-11 contracts.
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("contract {0} not found")]
    ContractNotFound(UInt160),

    #[error("contract {0} does not support the required standard")]
    NotSupported(UInt160),

    #[error("script invocation fault for method {method}: {message}")]
    InvocationFault {
        method: &'static str,
        message: String,
    },

    #[error("unexpected stack layout: {0}")]
    Stack(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error(transparent)]
    Script(#[from] ScriptHelperError),
}
