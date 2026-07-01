use neo_io::impl_serializable;
use serde::{Deserialize, Serialize};

/// Maximum data size (520 bytes)
const MAX_DATA_SIZE: usize = 520;

/// This message is sent to update the items for the BloomFilter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterAddPayload {
    /// The items to be added.
    pub data: Vec<u8>,
}

impl FilterAddPayload {
    /// Creates a new filter add payload.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl_serializable! {
    struct FilterAddPayload {
        data: var_bytes { max: MAX_DATA_SIZE },
    }
}
