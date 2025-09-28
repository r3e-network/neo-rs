// Copyright (C) 2015-2025 The Neo Project.
//
// get_blocks_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_io::{MemoryReader, Serializable};
use crate::UInt256;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// This message is sent to request for blocks by hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetBlocksPayload {
    /// The starting hash of the blocks to request.
    pub hash_start: UInt256,

    /// The number of blocks to request.
    pub count: i16,
}

impl GetBlocksPayload {
    /// Creates a new instance of the GetBlocksPayload class.
    /// Set count to -1 to request as many blocks as possible.
    pub fn create(hash_start: UInt256, count: i16) -> Self {
        Self { hash_start, count }
    }
}

impl Serializable for GetBlocksPayload {
    fn size(&self) -> usize {
        32 + 2 // UInt256 + i16
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.hash_start.serialize(writer)?;
        writer.write_all(&self.count.to_le_bytes())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let hash_start = UInt256::deserialize(reader)?;
        let count = reader.read_i16().map_err(|e| e.to_string())?;

        if count < -1 || count == 0 {
            return Err(format!("Invalid count: {}", count));
        }

        Ok(Self { hash_start, count })
    }
}
