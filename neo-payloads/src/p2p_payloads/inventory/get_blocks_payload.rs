use neo_io::{IoError, impl_serializable};
use neo_primitives::UInt256;
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
