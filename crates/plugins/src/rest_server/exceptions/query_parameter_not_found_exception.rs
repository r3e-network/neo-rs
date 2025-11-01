// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Exceptions.QueryParameterNotFoundException.

use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::models::error::error_model::ErrorModel;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct QueryParameterNotFoundException {
    message: String,
}

impl QueryParameterNotFoundException {
    pub fn new(parameter_name: &str) -> Self {
        Self {
            message: format!("Query parameter '{parameter_name}' was not found."),
        }
    }

    pub fn to_error_model(&self) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "QueryParameterNotFoundException".to_string(),
            self.message.clone(),
        )
    }
}
