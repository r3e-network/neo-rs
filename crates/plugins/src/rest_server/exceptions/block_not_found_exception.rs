// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.BlockNotFoundException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct BlockNotFoundException {
    message: String,
}

impl BlockNotFoundException {
    pub fn new(index: u32) -> Self {
        Self {
            message: format!("Block '{index}' was not found."),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "BlockNotFoundException".to_string(),
            self.message.clone(),
        )
    }
}
