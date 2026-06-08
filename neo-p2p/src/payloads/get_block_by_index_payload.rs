// Copyright (C) 2015-2025 The Neo Project.
//
// get_block_by_index_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms, with or without
// modifications are permitted.

use neo_io::{impl_serializable, IoError};
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

impl_serializable! {
    struct GetBlockByIndexPayload {
        index_start: u32,
        count: i16,
    }
    validate(self_ref) {
        if self_ref.count < -1 || self_ref.count == 0 || self_ref.count > MAX_HEADERS_COUNT {
            return Err(IoError::invalid_data("Invalid block count"));
        }
    }
}
