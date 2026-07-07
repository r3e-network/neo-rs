//! Address utility RPC operations.
//!
//! This module owns address-version validation against node settings. Request
//! parsing and JSON projection stay in the sibling request/response modules.

use serde_json::Value;

use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_utilities::response::validate_address_to_json;

impl RpcServer {
    /// Validate a Neo address against the node's configured address version.
    #[must_use]
    pub fn validate_address(&self, address: &str) -> Value {
        let address_version = self.system().settings().address_version;
        let is_valid =
            neo_wallets::wallet_helper::WalletAddress::to_script_hash(address, address_version)
                .is_ok();

        validate_address_to_json(address, is_valid)
    }
}
