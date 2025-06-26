//! RPC Methods
//!
//! Implementation of Neo N3 RPC methods.

use crate::types::*;
use neo_ledger::Ledger;
use neo_persistence::RocksDbStore;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, warn};

/// RPC methods implementation
#[derive(Clone)]
pub struct RpcMethods {
    ledger: Arc<Ledger>,
    storage: Arc<RocksDbStore>,
}

impl RpcMethods {
    /// Creates a new RpcMethods instance
    pub fn new(ledger: Arc<Ledger>, storage: Arc<RocksDbStore>) -> Self {
        Self { ledger, storage }
    }

    /// Gets the current block count
    pub async fn get_block_count(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getblockcount");
        let height = self.ledger.get_height();
        Ok(json!(height + 1)) // Neo returns count (height + 1)
    }

    /// Gets a block by hash or index
    pub async fn get_block(
        &self,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getblock with params: {:?}", params);

        let params = params.ok_or("Missing parameters")?;
        let params_array = params.as_array().ok_or("Invalid parameters format")?;

        if params_array.is_empty() {
            return Err("Missing block identifier".into());
        }

        // For now, return a mock block since we need to implement proper block retrieval
        let mock_block = RpcBlock {
            hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            size: 1024,
            version: 0,
            previous_block_hash:
                "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            merkle_root: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                .to_string(),
            time: 1640995200000,
            index: 0,
            next_consensus: "NiNmXL8FjEUEs1nfX9uHFBNaenxDHJtmuB".to_string(),
            witnesses: vec![],
            tx: vec![],
            confirmations: 1,
            next_block_hash: None,
        };

        Ok(serde_json::to_value(mock_block)?)
    }

    /// Gets block hash by index
    pub async fn get_block_hash(
        &self,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getblockhash with params: {:?}", params);

        let params = params.ok_or("Missing parameters")?;
        let params_array = params.as_array().ok_or("Invalid parameters format")?;

        if params_array.is_empty() {
            return Err("Missing block index".into());
        }

        let _index = params_array[0].as_u64().ok_or("Invalid block index")?;

        // Return mock hash for now
        Ok(json!(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ))
    }

    /// Gets the best block hash
    pub async fn get_best_block_hash(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getbestblockhash");
        // Return mock hash for now
        Ok(json!(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ))
    }

    /// Gets version information
    pub async fn get_version(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getversion");

        let version = RpcVersion {
            tcp_port: 20333,
            ws_port: 20334,
            nonce: rand::random_u32(),
            user_agent: "neo-rs/0.3.0".to_string(),
        };

        Ok(serde_json::to_value(version)?)
    }

    /// Gets peer information
    pub async fn get_peers(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getpeers");

        let peers = RpcPeers {
            unconnected: vec![
                RpcPeer {
                    address: "34.133.235.69".to_string(),
                    port: 20333,
                },
                RpcPeer {
                    address: "35.192.59.217".to_string(),
                    port: 20333,
                },
            ],
            bad: vec![],
            connected: vec![],
        };

        Ok(serde_json::to_value(peers)?)
    }

    /// Gets connection count
    pub async fn get_connection_count(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getconnectioncount");
        Ok(json!(0)) // Return 0 for now since we don't have active peer tracking in RPC
    }

    /// Validates an address
    pub async fn validate_address(
        &self,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: validateaddress with params: {:?}", params);

        let params = params.ok_or("Missing parameters")?;
        let params_array = params.as_array().ok_or("Invalid parameters format")?;

        if params_array.is_empty() {
            return Err("Missing address".into());
        }

        let address = params_array[0].as_str().ok_or("Invalid address format")?;

        // Basic validation - check if it looks like a Neo address
        let is_valid =
            address.len() == 34 && (address.starts_with('N') || address.starts_with('A'));

        let validation = RpcAddressValidation {
            address: address.to_string(),
            is_valid,
        };

        Ok(serde_json::to_value(validation)?)
    }

    /// Gets native contracts
    pub async fn get_native_contracts(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getnativecontracts");

        let contracts = vec![
            RpcNativeContract {
                id: -1,
                hash: "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5".to_string(),
                nef_checksum: "0x00000000".to_string(),
                update_counter: 0,
            },
            RpcNativeContract {
                id: -2,
                hash: "0x43cf98eddbe047e198a3e5d57006311442a0ca15".to_string(),
                nef_checksum: "0x00000000".to_string(),
                update_counter: 0,
            },
        ];

        Ok(serde_json::to_value(contracts)?)
    }
}

// Add random number generation
mod rand {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn random_u32() -> u32 {
        let mut hasher = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .hash(&mut hasher);
        hasher.finish() as u32
    }
}
