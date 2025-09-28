// Copyright (C) 2015-2025 The Neo Project.
//
// get_block_by_index_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

// Maximum headers count from HeadersPayload
const MAX_HEADERS_COUNT: i16 = 2000;

/// This message is sent to request for blocks by index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetBlockByIndexPayload {
    /// The starting index of the blocks to request.
    pub index_start: u32,

    /// The number of blocks to request.
    pub count: i16,
}

impl GetBlockByIndexPayload {
    /// Creates a new instance of the GetBlockByIndexPayload class.
    /// Set count to -1 to request as many blocks as possible.
    pub fn create(index_start: u32, count: i16) -> Self {
        Self { index_start, count }
    }
}

impl Serializable for GetBlockByIndexPayload {
    fn size(&self) -> usize {
        4 + 2 // u32 + i16
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.index_start.to_le_bytes())?;
        writer.write_all(&self.count.to_le_bytes())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let index_start = reader.read_u32().map_err(|e| e.to_string())?;
        let count = reader.read_i16().map_err(|e| e.to_string())?;

        if count < -1 || count == 0 || count > MAX_HEADERS_COUNT {
            return Err(format!("Invalid count: {}/{}", count, MAX_HEADERS_COUNT));
        }

        Ok(Self { index_start, count })
    }
}
