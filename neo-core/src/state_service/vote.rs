//! Vote message for StateService validation.
//!
//! Matches `Neo.Plugins.StateService.Network.Vote`.

use crate::neo_io::impl_serializable;

/// Vote payload carrying a validator signature for a state root.
#[derive(Debug, Clone)]
pub struct Vote {
    /// Validator index in the designated validator list.
    pub validator_index: i32,
    /// State root index.
    pub root_index: u32,
    /// Signature over the state root hash (64 bytes).
    pub signature: Vec<u8>,
}

impl_serializable! {
    struct Vote {
        validator_index: i32,
        root_index: u32,
        signature: var_bytes { max: 64 },
    }
}
