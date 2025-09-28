// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.ContractNotFoundException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::ErrorModel;
use neo_core::UInt160;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct ContractNotFoundException {
    message: String,
}

impl ContractNotFoundException {
    pub fn new(script_hash: UInt160) -> Self {
        Self {
            message: format!("Contract '{script_hash}' was not found."),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "ContractNotFoundException".to_string(),
            self.message.clone(),
        )
    }
}
