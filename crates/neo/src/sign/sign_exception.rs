// Copyright (C) 2015-2025 The Neo Project.
//
// sign_exception.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::error::Error;
use std::fmt;

/// The exception that is thrown when `Sign` fails.
#[derive(Debug, Clone)]
pub struct SignException {
    message: String,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl SignException {
    /// Initializes a new instance of the SignException class.
    ///
    /// # Arguments
    /// * `message` - The message that describes the error.
    pub fn new(message: String) -> Self {
        Self {
            message,
            cause: None,
        }
    }

    /// Initializes a new instance of the SignException class with a cause.
    ///
    /// # Arguments
    /// * `message` - The message that describes the error.
    /// * `cause` - The cause of the exception.
    pub fn with_cause(message: String, cause: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            message,
            cause: Some(cause),
        }
    }
}

impl fmt::Display for SignException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref cause) = self.cause {
            write!(f, " (caused by: {})", cause)?;
        }
        Ok(())
    }
}

impl Error for SignException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|e| e.as_ref() as &(dyn Error + 'static))
    }
}
