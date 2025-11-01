// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Models.Error.ParameterFormatExceptionModel`.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use serde::{Deserialize, Serialize};

/// Error payload emitted when request parameters fail validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterFormatExceptionModel {
    #[serde(flatten)]
    error: ErrorModel,
}

impl ParameterFormatExceptionModel {
    /// Creates a new model using the default message from the C# implementation.
    pub fn new() -> Self {
        Self {
            error: ErrorModel::with_params(
                RestErrorCodes::PARAMETER_FORMAT_EXCEPTION,
                "ParameterFormatException".to_string(),
                "Request parameter format is invalid.".to_string(),
            ),
        }
    }

    /// Creates a new model overriding the error message.
    pub fn with_message(message: impl Into<String>) -> Self {
        let mut model = Self::new();
        model.error.message = message.into();
        model
    }

    /// Converts the specialised model back into the plain [`ErrorModel`].
    pub fn into_error_model(self) -> ErrorModel {
        self.error
    }
}

impl Default for ParameterFormatExceptionModel {
    fn default() -> Self {
        Self::new()
    }
}
