// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! # Neo Constants
//!
//! Global constants used throughout the Neo blockchain implementation

pub const ADDRESS_SIZE: usize = 20;
pub const ADDRESS_VERSION: u8 = 0x35;
pub const HASH_SIZE: usize = 32;
pub const MAX_BLOCK_SIZE: usize = 4_194_304; // 4 MB
pub const MAX_SCRIPT_LENGTH: usize = 1_048_576; // 1 MB
pub const MAX_SCRIPT_SIZE: usize = MAX_SCRIPT_LENGTH;
pub const MAX_TRACEABLE_BLOCKS: u32 = 2_102_400; // 2_102_400 blocks (per ProtocolSettings.Default)
/// Maximum transactions allowed per block (matches Neo N3 protocol limit)
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 65_535; // u16::MAX
/// Maximum number of transactions retained in the mempool (ProtocolSettings.Default)
pub const MEMORY_POOL_MAX_TRANSACTIONS: usize = 50_000;
pub const MAX_TRANSACTION_SIZE: usize = 102_400; // 100 KB
pub const MILLISECONDS_PER_BLOCK: u64 = 15_000; // 15 seconds
pub const SECONDS_PER_BLOCK: u64 = 15;
/// Total initial GAS distribution (datoshi)
pub const INITIAL_GAS_DISTRIBUTION: u64 = 5_200_000_000_000_000;

// Additional constants specific to neo-core
/// Number of seconds in one hour
pub const SECONDS_PER_HOUR: u64 = 3600;
/// Number of milliseconds in one hour
pub const MILLISECONDS_PER_HOUR: u64 = SECONDS_PER_HOUR * 1000;
/// Genesis block timestamp in milliseconds (July 15, 2016 3:08:21 PM GMT)
pub const GENESIS_TIMESTAMP_MS: u64 = 1468595301000; // July 15, 2016 3:08:21 PM GMT
/// Size of one megabyte in bytes
pub const ONE_MEGABYTE: usize = 1048576;
/// Size of one kilobyte in bytes
pub const ONE_KILOBYTE: usize = 1024;

// Network constants
/// Default maximum number of peer connections
pub const DEFAULT_MAX_PEERS: usize = 100;
/// Default channel buffer size for message passing
pub const DEFAULT_CHANNEL_SIZE: usize = 1000;
/// Default timeout in milliseconds for network operations
pub const DEFAULT_TIMEOUT_MS: u64 = 5000;
/// Interval between ping messages in milliseconds (30 seconds)
pub const PING_INTERVAL_MS: u64 = 30000; // 30 seconds
/// Timeout for peer handshake in milliseconds (15 seconds)
pub const PEER_HANDSHAKE_TIMEOUT_MS: u64 = 15000; // 15 seconds

// Fee constants
/// Gas cost per byte of transaction data
pub const GAS_PER_BYTE: i64 = 1000;
/// Minimum network fee for a transaction
pub const MIN_NETWORK_FEE: i64 = 100000;

// Consensus constants
/// Maximum number of validators in the consensus
pub const MAX_VALIDATORS: usize = 21;
/// Minimum number of validators required for consensus
pub const MIN_VALIDATORS: usize = 4;
/// Maximum size of a consensus message in bytes
pub const CONSENSUS_MESSAGE_SIZE_LIMIT: usize = 1024;

// Storage constants
/// Cache size in megabytes for database operations
pub const CACHE_SIZE_MB: usize = 512;
/// Write buffer size in megabytes for database operations
pub const WRITE_BUFFER_SIZE_MB: usize = 64;
/// Maximum number of open files for database
pub const MAX_OPEN_FILES: i32 = 1000;
/// Whether to enable compression for RocksDB
pub const ROCKSDB_COMPRESSION_ENABLED: bool = true;

// VM constants
/// Maximum size of the execution stack
pub const MAX_STACK_SIZE: usize = 2048;
/// Maximum depth of invocation stack
pub const MAX_INVOCATION_STACK_SIZE: usize = 1024;
/// Maximum size of an array in the VM
pub const MAX_ARRAY_SIZE: usize = MAX_BLOCK_SIZE;
/// Maximum size of a single item in the VM
pub const MAX_ITEM_SIZE: usize = MAX_BLOCK_SIZE;

// RPC constants
/// Maximum size of an RPC request in bytes
pub const RPC_MAX_REQUEST_SIZE: usize = 10485760;
/// Maximum size of an RPC response in bytes
pub const RPC_MAX_RESPONSE_SIZE: usize = 10485760;
/// Default timeout for RPC requests in milliseconds (30 seconds)
pub const RPC_DEFAULT_TIMEOUT_MS: u64 = 30000; // 30 seconds

// P2P Protocol constants
/// Protocol version for P2P communication
pub const PROTOCOL_VERSION: u32 = 0;
/// Node capabilities bitmask
pub const NODE_CAPABILITIES: u32 = 1;

// TestNet configuration
/// Magic number for TestNet network identification
pub const TESTNET_MAGIC: u32 = 0x3554334E;
/// Default RPC port for TestNet
pub const TESTNET_RPC_PORT: u16 = 20332;
/// Default P2P port for TestNet
pub const TESTNET_P2P_PORT: u16 = 20333;

// MainNet configuration
/// Magic number for MainNet network identification (matches C# config.mainnet.json)
pub const MAINNET_MAGIC: u32 = 0x334F454E;
/// Default RPC port for MainNet
pub const MAINNET_RPC_PORT: u16 = 10332;
/// Default P2P port for MainNet
pub const MAINNET_P2P_PORT: u16 = 10333;

// Private network default ports
/// Default RPC port for private networks
pub const PRIVATE_NET_RPC_PORT: u16 = 30332;
/// Default P2P port for private networks
pub const PRIVATE_NET_P2P_PORT: u16 = 30333;

/// Storage limits
/// Maximum size of a storage key in bytes
pub const MAX_STORAGE_KEY_SIZE: usize = 64;
/// Maximum size of a storage value in bytes
pub const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;

/// Network retry configuration
pub const MAX_RETRY_ATTEMPTS: u32 = 3;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_time_constants() {
        assert_eq!(MILLISECONDS_PER_BLOCK, 15000);
        assert_eq!(MILLISECONDS_PER_HOUR, 3600000);
    }
    #[test]
    fn test_size_constants() {
        assert_eq!(MAX_BLOCK_SIZE, 4_194_304); // 4MB as defined in neo-config
        assert_eq!(MAX_TRANSACTION_SIZE, 102_400); // 100KB as defined in neo-config
    }
}
