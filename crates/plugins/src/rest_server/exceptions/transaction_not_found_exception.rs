// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.TransactionNotFoundException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use neo_core::UInt256;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct TransactionNotFoundException {
    message: String,
}

impl TransactionNotFoundException {
    pub fn new(hash: UInt256) -> Self {
        Self {
            message: format!("Transaction '{hash}' was not found."),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "TransactionNotFoundException".to_string(),
            self.message.clone(),
        )
    }
}
