use neo_io::{IoError, impl_serializable};
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
