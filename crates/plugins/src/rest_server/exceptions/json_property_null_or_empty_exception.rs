// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.JsonPropertyNullOrEmptyException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct JsonPropertyNullOrEmptyException {
    message: String,
}

impl JsonPropertyNullOrEmptyException {
    pub fn new() -> Self {
        Self::with_param_name("value")
    }

    pub fn with_param_name(name: &str) -> Self {
        Self {
            message: format!("Value cannot be null or empty. (Parameter '{name}')"),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::PARAMETER_FORMAT_EXCEPTION,
            "JsonPropertyNullOrEmptyException".to_string(),
            self.message.clone(),
        )
    }
}
