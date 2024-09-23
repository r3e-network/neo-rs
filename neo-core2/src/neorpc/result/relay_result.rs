use serde::Serialize;
use crate::util::Uint256;

// RelayResult is a result of `sendrawtransaction` or `submitblock` RPC calls.
#[derive(Serialize)]
pub struct RelayResult {
    pub hash: Uint256,
}
