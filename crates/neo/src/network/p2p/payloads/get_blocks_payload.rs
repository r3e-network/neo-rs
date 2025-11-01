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

use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::UInt256;
use serde::{Deserialize, Serialize};

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

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.hash_start, writer)?;
        writer.write_i16(self.count)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let hash_start = <UInt256 as Serializable>::deserialize(reader)?;
        let count = reader.read_i16()?;

        if count < -1 || count == 0 {
            return Err(IoError::invalid_data("Invalid count"));
        }

        Ok(Self { hash_start, count })
    }
}
