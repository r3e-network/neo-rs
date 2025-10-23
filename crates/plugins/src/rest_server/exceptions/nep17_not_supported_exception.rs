// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.Nep17NotSupportedException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use neo_core::UInt160;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct Nep17NotSupportedException {
    message: String,
}

impl Nep17NotSupportedException {
    pub fn new(script_hash: UInt160) -> Self {
        Self {
            message: format!("Contract '{script_hash}' does not support NEP-17."),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "Nep17NotSupportedException".to_string(),
            self.message.clone(),
        )
    }
}
