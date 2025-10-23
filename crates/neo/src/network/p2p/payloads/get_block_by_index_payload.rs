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

use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

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

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.index_start)?;
        writer.write_i16(self.count)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let index_start = reader.read_u32()?;
        let count = reader.read_i16()?;

        if count < -1 || count == 0 || count > MAX_HEADERS_COUNT {
            return Err(IoError::invalid_data("Invalid block count"));
        }

        Ok(Self { index_start, count })
    }
}
