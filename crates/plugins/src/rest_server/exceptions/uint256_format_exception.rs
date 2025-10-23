// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.UInt256FormatException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct UInt256FormatException {
    message: String,
}

impl UInt256FormatException {
    pub fn new() -> Self {
        Self::with_message("Invalid UInt256 format.")
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::PARAMETER_FORMAT_EXCEPTION,
            "UInt256FormatException".to_string(),
            self.message.clone(),
        )
    }
}
