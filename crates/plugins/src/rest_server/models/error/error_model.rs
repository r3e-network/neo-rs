// Copyright (C) 2015-2025 The Neo Project.
//
// error_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Error model matching C# ErrorModel exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorModel {
    /// Error's HResult Code
    /// Matches C# Code property
    pub code: i32,

    /// Error's name of the type
    /// Matches C# Name property
    pub name: String,

    /// Error's exception message
    /// Matches C# Message property
    pub message: String,
}

impl ErrorModel {
    /// Creates a new ErrorModel
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            code: 1000,
            name: "GeneralException".to_string(),
            message: "An error occurred.".to_string(),
        }
    }

    /// Creates a new ErrorModel with parameters
    /// Matches C# constructor with parameters
    pub fn with_params(code: i32, name: String, message: String) -> Self {
        Self {
            code,
            name,
            message,
        }
    }
}

impl Default for ErrorModel {
    fn default() -> Self {
        Self::new()
    }
}
