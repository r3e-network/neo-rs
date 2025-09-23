//! Port stub for `Utility.cs` RPC helpers.

use neo_core::UInt160;

#[derive(Debug, Default)]
pub struct RpcUtility;

impl RpcUtility {
    pub fn format_address(hash: &UInt160) -> String {
        hash.to_string()
    }
}
