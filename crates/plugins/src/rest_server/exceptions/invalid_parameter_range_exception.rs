// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.InvalidParameterRangeException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct InvalidParameterRangeException {
    message: String,
}

impl InvalidParameterRangeException {
    pub fn new() -> Self {
        Self::with_message("One or more parameters are outside the allowed range.")
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::PARAMETER_FORMAT_EXCEPTION,
            "InvalidParameterRangeException".to_string(),
            self.message.clone(),
        )
    }
}
