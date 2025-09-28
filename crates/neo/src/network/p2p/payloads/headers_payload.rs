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
use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Indicates the maximum number of headers sent each time.
pub const MAX_HEADERS_COUNT: usize = 2000;

/// This message is sent to respond to GetHeaders messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        1 + self.headers.iter().map(|h| h.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        if self.headers.len() > MAX_HEADERS_COUNT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Too many headers",
            ));
        }

        writer.write_all(&[self.headers.len() as u8])?;
        for header in &self.headers {
            header.serialize(writer)?;
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let count = reader.read_var_int().map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Empty headers list".to_string());
        }
        if count > MAX_HEADERS_COUNT as u64 {
            return Err("Too many headers".to_string());
        }

        let mut headers = Vec::with_capacity(count as usize);
        for _ in 0..count {
            headers.push(Header::deserialize(reader)?);
        }

        Ok(Self { headers })
    }
}
