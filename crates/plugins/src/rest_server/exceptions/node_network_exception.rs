// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.NodeNetworkException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct NodeNetworkException {
    message: String,
}

impl NodeNetworkException {
    pub fn new() -> Self {
        Self::with_message("Network does not match config file's.")
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "NodeNetworkException".to_string(),
            self.message.clone(),
        )
    }
}
