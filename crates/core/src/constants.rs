// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! # Neo Constants
//!
//! Global constants used throughout the Neo blockchain implementation

// Re-export constants from neo-config
pub use neo_config::{
    ADDRESS_SIZE, MAX_BLOCK_SIZE, MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE, MAX_TRACEABLE_BLOCKS,
    MAX_TRANSACTIONS_PER_BLOCK, MAX_TRANSACTION_SIZE, MILLISECONDS_PER_BLOCK, SECONDS_PER_BLOCK,
};

// Additional constants specific to neo-core
pub const SECONDS_PER_HOUR: u64 = 3600;
pub const MILLISECONDS_PER_HOUR: u64 = SECONDS_PER_HOUR * 1000;
pub const GENESIS_TIMESTAMP_MS: u64 = 1468595301000; // July 15, 2016 3:08:21 PM GMT
pub const ONE_MEGABYTE: usize = 1048576;
pub const ONE_KILOBYTE: usize = 1024;

// Network constants
pub const DEFAULT_MAX_PEERS: usize = 100;
pub const DEFAULT_CHANNEL_SIZE: usize = 1000;
pub const DEFAULT_TIMEOUT_MS: u64 = 5000;
pub const PING_INTERVAL_MS: u64 = 30000; // 30 seconds
pub const PEER_HANDSHAKE_TIMEOUT_MS: u64 = 15000; // 15 seconds

// Fee constants
pub const GAS_PER_BYTE: i64 = 1000;
pub const MIN_NETWORK_FEE: i64 = 100000;

// Consensus constants
pub const MAX_VALIDATORS: usize = 21;
pub const MIN_VALIDATORS: usize = 4;
pub const CONSENSUS_MESSAGE_SIZE_LIMIT: usize = 1024;

// Storage constants
pub const CACHE_SIZE_MB: usize = 512;
pub const WRITE_BUFFER_SIZE_MB: usize = 64;
pub const MAX_OPEN_FILES: i32 = 1000;
pub const ROCKSDB_COMPRESSION_ENABLED: bool = true;

// VM constants
pub const MAX_STACK_SIZE: usize = 2048;
pub const MAX_INVOCATION_STACK_SIZE: usize = 1024;
pub const MAX_ARRAY_SIZE: usize = MAX_BLOCK_SIZE;
pub const MAX_ITEM_SIZE: usize = MAX_BLOCK_SIZE;

// RPC constants
pub const RPC_MAX_REQUEST_SIZE: usize = 10485760;
pub const RPC_MAX_RESPONSE_SIZE: usize = 10485760;
pub const RPC_DEFAULT_TIMEOUT_MS: u64 = 30000; // 30 seconds

// P2P Protocol constants
pub const PROTOCOL_VERSION: u32 = 0;
pub const NODE_CAPABILITIES: u32 = 1;

// TestNet configuration
pub const TESTNET_MAGIC: u32 = 0x3554334E;
pub const TESTNET_RPC_PORT: u16 = 20332;
pub const TESTNET_P2P_PORT: u16 = 20333;

// MainNet configuration
pub const MAINNET_MAGIC: u32 = 0x4e454f4e;
pub const MAINNET_RPC_PORT: u16 = 10332;
pub const MAINNET_P2P_PORT: u16 = 10333;

// Private network default ports
pub const PRIVATE_NET_RPC_PORT: u16 = 30332;
pub const PRIVATE_NET_P2P_PORT: u16 = 30333;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_constants() {
        assert_eq!(MILLISECONDS_PER_BLOCK, 15000);
        assert_eq!(MILLISECONDS_PER_HOUR, 3600000);
    }

    #[test]
    fn test_size_constants() {
        assert_eq!(MAX_BLOCK_SIZE, 1_048_576);
        assert_eq!(MAX_TRANSACTION_SIZE, 102_400);
    }
}

/// Storage limits
pub const MAX_STORAGE_KEY_SIZE: usize = 64;
pub const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;

/// Network retry configuration
pub const MAX_RETRY_ATTEMPTS: u32 = 3;
