use serde::Serialize;
use crate::util::Uint256;

// RawMempool represents a result of getrawmempool RPC call.
#[derive(Serialize)]
pub struct RawMempool {
    pub height: u32,
    pub verified: Vec<Uint256>,
    pub unverified: Vec<Uint256>,
}
