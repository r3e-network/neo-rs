//! Protocol constants for Neo blockchain.
//!
//! These constants define fundamental sizes and limits used throughout the protocol.
//! Matches C# Neo constants exactly.

// === Size Constants ===

/// Size of an address/script hash in bytes (160 bits = 20 bytes).
pub const ADDRESS_SIZE: usize = 20;

/// Size of a hash in bytes (256 bits = 32 bytes).
pub const HASH_SIZE: usize = 32;

/// Size of one megabyte in bytes.
pub const ONE_MEGABYTE: usize = 1_048_576;

/// Size of one kilobyte in bytes.
pub const ONE_KILOBYTE: usize = 1024;

// === Block Constants ===

/// Maximum size of a block in bytes (2 MB).
pub const MAX_BLOCK_SIZE: usize = 2_097_152;

/// Maximum transactions allowed per block.
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 512;

/// Maximum number of traceable blocks (per ProtocolSettings.Default).
pub const MAX_TRACEABLE_BLOCKS: u32 = 2_102_400;

// === Transaction Constants ===

/// Maximum size of a transaction in bytes (100 KB).
pub const MAX_TRANSACTION_SIZE: usize = 102_400;

/// Maximum number of attributes per transaction.
pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;

/// Maximum number of cosigners per transaction.
pub const MAX_COSIGNERS: usize = 16;

// === Script Constants ===

/// Maximum size of a script in bytes (1 MB).
pub const MAX_SCRIPT_SIZE: usize = 1_048_576;

/// Maximum size of a script in bytes (alias for MAX_SCRIPT_SIZE).
pub const MAX_SCRIPT_LENGTH: usize = MAX_SCRIPT_SIZE;

// === Time Constants ===

/// Seconds per block (default for Neo N3).
pub const SECONDS_PER_BLOCK: u64 = 15;

/// Milliseconds per block (15 seconds).
pub const MILLISECONDS_PER_BLOCK: u64 = 15_000;

/// Number of seconds in one hour.
pub const SECONDS_PER_HOUR: u64 = 3600;

/// Number of milliseconds in one hour.
pub const MILLISECONDS_PER_HOUR: u64 = SECONDS_PER_HOUR * 1000;

/// Genesis block timestamp in milliseconds (July 15, 2016 3:08:21 PM GMT).
pub const GENESIS_TIMESTAMP_MS: u64 = 1_468_595_301_000;

// === Address Constants ===

/// Neo N3 address version byte.
pub const ADDRESS_VERSION: u8 = 0x35;

// === Fee Constants ===

/// Gas cost per byte of transaction data.
pub const GAS_PER_BYTE: i64 = 1000;

/// Minimum network fee for a transaction.
pub const MIN_NETWORK_FEE: i64 = 100_000;

/// Total initial GAS distribution (datoshi).
pub const INITIAL_GAS_DISTRIBUTION: u64 = 5_200_000_000_000_000;

// === Network Constants ===

/// Default maximum number of peer connections.
pub const DEFAULT_MAX_PEERS: usize = 100;

/// Default channel buffer size for message passing.
pub const DEFAULT_CHANNEL_SIZE: usize = 1000;

/// Default timeout in milliseconds for network operations.
pub const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Interval between ping messages in milliseconds (30 seconds).
pub const PING_INTERVAL_MS: u64 = 30_000;

/// Timeout for peer handshake in milliseconds (15 seconds).
pub const PEER_HANDSHAKE_TIMEOUT_MS: u64 = 15_000;

/// Maximum number of transactions retained in the mempool (ProtocolSettings.Default).
pub const MEMORY_POOL_MAX_TRANSACTIONS: usize = 50_000;

// === Consensus Constants ===

/// Maximum number of validators in the consensus.
pub const MAX_VALIDATORS: usize = 21;

/// Minimum number of validators required for consensus.
pub const MIN_VALIDATORS: usize = 4;

/// Maximum size of a consensus message in bytes.
pub const CONSENSUS_MESSAGE_SIZE_LIMIT: usize = 1024;

// === Storage Constants ===

/// Cache size in megabytes for database operations.
pub const CACHE_SIZE_MB: usize = 512;

/// Write buffer size in megabytes for database operations.
pub const WRITE_BUFFER_SIZE_MB: usize = 64;

/// Maximum number of open files for database.
pub const MAX_OPEN_FILES: i32 = 1000;

/// Whether to enable compression for RocksDB.
pub const ROCKSDB_COMPRESSION_ENABLED: bool = true;

/// Maximum size of a storage key in bytes.
pub const MAX_STORAGE_KEY_SIZE: usize = 64;

/// Maximum size of a storage value in bytes.
pub const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;

// === VM Constants ===

/// Maximum size of the execution stack.
pub const MAX_STACK_SIZE: usize = 2048;

/// Maximum depth of invocation stack.
pub const MAX_INVOCATION_STACK_SIZE: usize = 1024;

/// Maximum size of an array in the VM.
pub const MAX_ARRAY_SIZE: usize = MAX_BLOCK_SIZE;

/// Maximum size of a single item in the VM.
pub const MAX_ITEM_SIZE: usize = MAX_BLOCK_SIZE;

// === RPC Constants ===

/// Maximum size of an RPC request in bytes (10 MB).
pub const RPC_MAX_REQUEST_SIZE: usize = 10_485_760;

/// Maximum size of an RPC response in bytes (10 MB).
pub const RPC_MAX_RESPONSE_SIZE: usize = 10_485_760;

/// Default timeout for RPC requests in milliseconds (30 seconds).
pub const RPC_DEFAULT_TIMEOUT_MS: u64 = 30_000;

// === P2P Protocol Constants ===

/// Protocol version for P2P communication.
pub const PROTOCOL_VERSION: u32 = 0;

/// Node capabilities bitmask.
pub const NODE_CAPABILITIES: u32 = 1;

/// Network retry configuration.
pub const MAX_RETRY_ATTEMPTS: u32 = 3;

// === Network Magic Numbers ===

/// Magic number for TestNet network identification.
pub const TESTNET_MAGIC: u32 = 0x3554334E;

/// Default RPC port for TestNet.
pub const TESTNET_RPC_PORT: u16 = 20332;

/// Default P2P port for TestNet.
pub const TESTNET_P2P_PORT: u16 = 20333;

/// Magic number for MainNet network identification (matches C# config.mainnet.json).
pub const MAINNET_MAGIC: u32 = 0x334F454E;

/// Default RPC port for MainNet.
pub const MAINNET_RPC_PORT: u16 = 10332;

/// Default P2P port for MainNet.
pub const MAINNET_P2P_PORT: u16 = 10333;

/// Default RPC port for private networks.
pub const PRIVATE_NET_RPC_PORT: u16 = 30332;

/// Default P2P port for private networks.
pub const PRIVATE_NET_P2P_PORT: u16 = 30333;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_constants() {
        assert_eq!(ADDRESS_SIZE, 20);
        assert_eq!(HASH_SIZE, 32);
        assert_eq!(ONE_MEGABYTE, 1_048_576);
        assert_eq!(ONE_KILOBYTE, 1024);
    }

    #[test]
    fn test_block_constants() {
        assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2MB
        assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512);
        assert_eq!(MAX_TRACEABLE_BLOCKS, 2_102_400);
    }

    #[test]
    fn test_transaction_constants() {
        assert_eq!(MAX_TRANSACTION_SIZE, 102_400); // 100KB
        assert_eq!(MAX_TRANSACTION_ATTRIBUTES, 16);
        assert_eq!(MAX_COSIGNERS, 16);
    }

    #[test]
    fn test_time_constants() {
        assert_eq!(SECONDS_PER_BLOCK, 15);
        assert_eq!(MILLISECONDS_PER_BLOCK, 15_000);
        assert_eq!(MILLISECONDS_PER_HOUR, 3_600_000);
    }

    #[test]
    fn test_network_magic() {
        assert_eq!(TESTNET_MAGIC, 0x3554334E);
        assert_eq!(MAINNET_MAGIC, 0x334F454E);
    }

    #[test]
    fn test_port_constants() {
        assert_eq!(TESTNET_RPC_PORT, 20332);
        assert_eq!(TESTNET_P2P_PORT, 20333);
        assert_eq!(MAINNET_RPC_PORT, 10332);
        assert_eq!(MAINNET_P2P_PORT, 10333);
    }

    #[test]
    fn test_fee_constants() {
        assert_eq!(GAS_PER_BYTE, 1000);
        assert_eq!(MIN_NETWORK_FEE, 100_000);
    }

    #[test]
    fn test_consensus_constants() {
        assert_eq!(MAX_VALIDATORS, 21);
        assert_eq!(MIN_VALIDATORS, 4);
    }

    #[test]
    fn test_vm_constants() {
        assert_eq!(MAX_STACK_SIZE, 2048);
        assert_eq!(MAX_INVOCATION_STACK_SIZE, 1024);
    }
}
