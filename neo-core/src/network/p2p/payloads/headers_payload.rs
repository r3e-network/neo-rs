// Copyright (C) 2015-2025 The Neo Project.
//
// headers_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::header::Header;
use crate::neo_io::{impl_serializable, IoError};
use serde::{Deserialize, Serialize};

/// Indicates the maximum number of headers sent each time.
pub const MAX_HEADERS_COUNT: usize = 2000;

/// This message is sent to respond to GetHeaders messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadersPayload {
    /// The list of headers.
    pub headers: Vec<Header>,
}

impl HeadersPayload {
    /// Creates a new headers payload.
    pub fn create(headers: Vec<Header>) -> Self {
        Self { headers }
    }
}

impl_serializable! {
    struct HeadersPayload {
        headers: var_array<Header> { max: MAX_HEADERS_COUNT },
    }
    validate(self_ref) {
        if self_ref.headers.is_empty() {
            return Err(IoError::invalid_data("Empty headers list"));
        }
    }
}
