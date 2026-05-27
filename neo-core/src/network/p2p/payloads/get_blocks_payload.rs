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

use crate::neo_io::{impl_serializable, IoError};
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

impl_serializable! {
    struct GetBlocksPayload {
        hash_start: UInt256,
        count: i16,
    }
    validate(self_ref) {
        if self_ref.count < -1 || self_ref.count == 0 {
            return Err(IoError::invalid_data("Invalid count"));
        }
    }
}
