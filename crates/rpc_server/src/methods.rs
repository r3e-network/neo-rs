//! RPC Methods
//!
//! Implementation of Neo N3 RPC methods.

use hex;
use neo_config::HASH_SIZE;
use neo_ledger::Ledger;
use neo_persistence::RocksDbStore;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, warn};

// Define Error and Result types locally
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

// Import RPC types from rpc_client
use neo_rpc_client::models::{
    RpcAddressValidation, RpcBlock, RpcPeer, RpcPeers, RpcSigner, RpcTransaction, RpcVersion,
    RpcWitness,
};

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

    /// Converts a Block to RpcBlock
    fn block_to_rpc_block(
        &self,
        block: &neo_ledger::Block,
        confirmations: u32,
        next_block_hash: Option<String>,
    ) -> RpcBlock {
        RpcBlock {
            hash: format!("0x{}", hex::encode(block.hash().as_bytes())),
            size: block.size() as u32,
            version: block.header.version,
            previous_block_hash: format!(
                "0x{}",
                hex::encode(block.header.previous_hash.as_bytes())
            ),
            merkle_root: format!("0x{}", hex::encode(block.header.merkle_root.as_bytes())),
            time: block.header.timestamp,
            index: block.header.index,
            next_consensus: block.header.next_consensus.to_string(),
            witnesses: block
                .header
                .witnesses
                .iter()
                .map(|w| RpcWitness {
                    invocation: hex::encode(&w.invocation_script),
                    verification: hex::encode(&w.verification_script),
                })
                .collect(),
            tx: block
                .transactions
                .iter()
                .map(|tx| RpcTransaction {
                    hash: match tx.hash() {
                        Ok(hash) => format!("0x{}", hex::encode(hash.as_bytes())),
                        Err(_) => {
                            "0x0000000000000000000000000000000000000000000000000000000000000000"
                                .to_string()
                        }
                    },
                    size: tx.size() as u32,
                    version: tx.version(),
                    nonce: tx.nonce(),
                    system_fee: tx.system_fee().to_string(),
                    network_fee: tx.network_fee().to_string(),
                    valid_until_block: tx.valid_until_block(),
                    signers: tx
                        .signers()
                        .iter()
                        .map(|s| RpcSigner {
                            account: format!("0x{}", hex::encode(s.account.as_bytes())),
                            scopes: s.scopes.to_string(),
                        })
                        .collect(),
                    attributes: tx
                        .attributes()
                        .iter()
                        .map(|attr| match attr {
                            neo_core::TransactionAttribute::HighPriority => {
                                serde_json::json!({"type": "HighPriority"})
                            }
                            neo_core::TransactionAttribute::OracleResponse { id, code, result } => {
                                serde_json::json!({
                                    "type": "OracleResponse",
                                    "id": id,
                                    "code": *code as u8,
                                    "result": hex::encode(result)
                                })
                            }
                            neo_core::TransactionAttribute::NotValidBefore { height } => {
                                serde_json::json!({
                                    "type": "NotValidBefore",
                                    "height": height
                                })
                            }
                            neo_core::TransactionAttribute::Conflicts { hash } => {
                                serde_json::json!({
                                    "type": "Conflicts",
                                    "hash": format!("0x{}", hex::encode(hash.as_bytes()))
                                })
                            }
                        })
                        .collect(),
                    script: hex::encode(tx.script()),
                    witnesses: tx
                        .witnesses()
                        .iter()
                        .map(|w| RpcWitness {
                            invocation: hex::encode(&w.invocation_script),
                            verification: hex::encode(&w.verification_script),
                        })
                        .collect(),
                })
                .collect(),
            confirmations,
            next_block_hash,
        }
    }

    /// Gets the current block count
    pub async fn get_block_count(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getblockcount");
        let height = self.ledger.get_height().await;
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

        let block_param = &params_array[0];

        let block = if let Some(hash_str) = block_param.as_str() {
            // Try to parse as hex hash
            if let Ok(hash_bytes) = hex::decode(hash_str.trim_start_matches("0x")) {
                if hash_bytes.len() == HASH_SIZE {
                    let mut bytes = [0u8; HASH_SIZE];
                    bytes.copy_from_slice(&hash_bytes);
                    let hash = neo_core::UInt256::from(bytes);
                    self.ledger.get_block_by_hash(&hash).await?
                } else {
                    return Err("Invalid hash format".into());
                }
            } else {
                return Err("Invalid hash format".into());
            }
        } else if let Some(index) = block_param.as_u64() {
            // Parse as block index
            self.ledger.get_block(index as u32).await?
        } else {
            return Err("Invalid block identifier format".into());
        };

        match block {
            Some(block) => {
                // Calculate confirmations
                let current_height = self.ledger.get_height().await;
                let confirmations = if current_height >= block.header.index {
                    current_height - block.header.index + 1
                } else {
                    0
                };

                let next_block_hash = if block.header.index < current_height {
                    match self.ledger.get_block(block.header.index + 1).await? {
                        Some(next_block) => {
                            Some(format!("0x{}", hex::encode(next_block.hash().as_bytes())))
                        }
                        None => None,
                    }
                } else {
                    None
                };

                let rpc_block = self.block_to_rpc_block(&block, confirmations, next_block_hash);
                Ok(serde_json::to_value(rpc_block)?)
            }
            None => Err("Block not found".into()),
        }
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

        let index = params_array[0].as_u64().ok_or("Invalid block index")? as u32;

        // Get the actual block and return its hash
        match self.ledger.get_block(index).await? {
            Some(block) => {
                let hash_hex = hex::encode(block.hash().as_bytes());
                Ok(json!(format!("0x{}", hash_hex)))
            }
            None => Err("Block not found".into()),
        }
    }

    /// Gets the best block hash
    pub async fn get_best_block_hash(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getbestblockhash");
        let best_hash = self.ledger.get_best_block_hash().await?;
        let hash_hex = hex::encode(best_hash.as_bytes());
        Ok(json!(format!("0x{}", hash_hex)))
    }

    /// Gets version information
    pub async fn get_version(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getversion");

        // Get version from environment at compile time
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        const NEO_VERSION: &str = "3.6.0";

        let version = RpcVersion {
            tcp_port: 20333,
            ws_port: 20334,
            nonce: rand::random_u32(),
            user_agent: format!("neo-rs/{}", VERSION),
        };

        Ok(serde_json::to_value(version)?)
    }

    /// Gets peer information
    pub async fn get_peers(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        debug!("RPC: getpeers");

        let peers = RpcPeers {
            unconnected: vec![
                RpcPeer {
                    address: "seed1.neo.org".to_string(),
                    port: 20333,
                },
                RpcPeer {
                    address: "seed2.neo.org".to_string(),
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
        // Get actual connection count from network manager
        let connection_count = if let Some(network) = &self.network {
            network.get_connected_peer_count()
        } else {
            0
        };
        Ok(json!(connection_count))
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
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        hasher.finish() as u32
    }
}
