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
use crate::neo_io::serializable::helper::{
    deserialize_array, get_var_size_serializable_slice, serialize_array,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
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

impl Serializable for HeadersPayload {
    fn size(&self) -> usize {
        get_var_size_serializable_slice(&self.headers)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.headers.len() > MAX_HEADERS_COUNT {
            return Err(IoError::invalid_data("Too many headers"));
        }

        serialize_array(&self.headers, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let headers = deserialize_array(reader, MAX_HEADERS_COUNT)?;
        if headers.is_empty() {
            return Err(IoError::invalid_data("Empty headers list"));
        }

        Ok(Self { headers })
    }
}
