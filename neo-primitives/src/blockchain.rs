//! Blockchain and P2P service traits for Neo blockchain.
//!
//! This module provides traits for blockchain access and peer management,
//! breaking the circular dependency between neo-p2p and neo-core
//! (Chain 3: `LocalNode` → Blockchain ↔ `PeerManagerService`).
//!
//! # Design
//!
//! - `BlockchainProvider`: Query and relay operations for blockchain
//! - `PeerRegistry`: Peer management and message broadcasting
//! - `IMessage`, `IBlock`, `IHeader`: Marker traits for associated types
//!
//! # Example
//!
//! ```rust,ignore
//! use neo_primitives::{BlockchainProvider, PeerRegistry};
//! use std::sync::Arc;
//!
//! // LocalNode can be generic over these traits
//! struct LocalNode<B, P>
//! where
//!     B: BlockchainProvider,
//!     P: PeerRegistry,
//! {
//!     blockchain: Arc<B>,
//!     peers: Arc<P>,
//! }
//! ```

use crate::{UInt160, UInt256};
use thiserror::Error;

// ============ Error Types ============

/// Errors that can occur during block/transaction relay.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum RelayError {
    /// Block validation failed.
    #[error("block validation failed: {message}")]
    ValidationFailed {
        /// Detailed error message.
        message: String,
    },

    /// Block already exists in the blockchain.
    #[error("block already exists: {hash}")]
    AlreadyExists {
        /// Hash of the existing block.
        hash: String,
    },

    /// Transaction validation failed.
    #[error("transaction invalid: {message}")]
    TransactionInvalid {
        /// Detailed error message.
        message: String,
    },

    /// Memory pool is full.
    #[error("memory pool full: size={current}, max={max}")]
    MempoolFull {
        /// Current mempool size.
        current: usize,
        /// Maximum mempool size.
        max: usize,
    },

    /// Block height is invalid.
    #[error("invalid block height: expected={expected}, got={got}")]
    InvalidHeight {
        /// Expected block height.
        expected: u32,
        /// Actual block height.
        got: u32,
    },
}

impl RelayError {
    /// Create a validation failed error.
    pub fn validation_failed<S: Into<String>>(message: S) -> Self {
        Self::ValidationFailed {
            message: message.into(),
        }
    }

    /// Create an already exists error.
    #[must_use]
    pub fn already_exists(hash: &UInt256) -> Self {
        Self::AlreadyExists {
            hash: format!("{hash:?}"),
        }
    }

    /// Create a transaction invalid error.
    pub fn transaction_invalid<S: Into<String>>(message: S) -> Self {
        Self::TransactionInvalid {
            message: message.into(),
        }
    }

    /// Create a mempool full error.
    #[must_use]
    pub const fn mempool_full(current: usize, max: usize) -> Self {
        Self::MempoolFull { current, max }
    }

    /// Create an invalid height error.
    #[must_use]
    pub const fn invalid_height(expected: u32, got: u32) -> Self {
        Self::InvalidHeight { expected, got }
    }
}

/// Errors that can occur during peer communication.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Peer not found.
    #[error("peer not found: {id}")]
    PeerNotFound {
        /// Peer ID that was not found.
        id: u64,
    },

    /// Peer is disconnected.
    #[error("peer disconnected: {id}")]
    Disconnected {
        /// Peer ID that is disconnected.
        id: u64,
    },

    /// Send queue is full.
    #[error("send queue full for peer {id}")]
    QueueFull {
        /// Peer ID whose queue is full.
        id: u64,
    },

    /// Serialization error.
    #[error("message serialization failed: {message}")]
    SerializationFailed {
        /// Detailed error message.
        message: String,
    },
}

impl SendError {
    /// Create a peer not found error.
    #[must_use]
    pub const fn peer_not_found(id: u64) -> Self {
        Self::PeerNotFound { id }
    }

    /// Create a disconnected error.
    #[must_use]
    pub const fn disconnected(id: u64) -> Self {
        Self::Disconnected { id }
    }

    /// Create a queue full error.
    #[must_use]
    pub const fn queue_full(id: u64) -> Self {
        Self::QueueFull { id }
    }

    /// Create a serialization failed error.
    pub fn serialization_failed<S: Into<String>>(message: S) -> Self {
        Self::SerializationFailed {
            message: message.into(),
        }
    }
}

/// Result type for relay operations.
pub type RelayResult<T> = Result<T, RelayError>;

/// Result type for send operations.
pub type SendResult<T> = Result<T, SendError>;

// ============ Peer Types ============

/// Unique identifier for a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

impl PeerId {
    /// Create a new peer ID.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner ID value.
    #[must_use]
    pub const fn inner(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({})", self.0)
    }
}

/// Information about a connected peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Unique peer identifier.
    pub id: PeerId,
    /// Remote address (IP:port).
    pub address: String,
    /// Protocol version.
    pub version: u32,
    /// Unix timestamp when connected.
    pub connected_at: u64,
    /// Start height reported by peer.
    pub start_height: u32,
    /// User agent string.
    pub user_agent: String,
}

impl PeerInfo {
    /// Create new peer info.
    #[must_use]
    pub const fn new(
        id: PeerId,
        address: String,
        version: u32,
        connected_at: u64,
        start_height: u32,
        user_agent: String,
    ) -> Self {
        Self {
            id,
            address,
            version,
            connected_at,
            start_height,
            user_agent,
        }
    }
}

// ============ Marker Traits ============

/// Trait for network messages.
///
/// Implementations should provide serialization for network transmission.
pub trait IMessage: Send + Sync {
    /// Returns the command name for this message type.
    fn command(&self) -> &str;

    /// Serializes the message to bytes.
    fn serialize(&self) -> Vec<u8>;
}

/// Trait for block data.
///
/// Provides common operations on blocks without exposing internal structure.
pub trait IBlock: Send + Sync {
    /// Associated type for transactions in this block.
    type Transaction;

    /// Returns the block hash.
    fn hash(&self) -> UInt256;

    /// Returns the block index (height).
    fn index(&self) -> u32;

    /// Returns the block timestamp.
    fn timestamp(&self) -> u64;

    /// Returns the previous block hash.
    fn prev_hash(&self) -> UInt256;

    /// Returns the merkle root of transactions.
    fn merkle_root(&self) -> UInt256;

    /// Returns the number of transactions.
    fn transaction_count(&self) -> usize;
}

/// Trait for block header data.
///
/// Headers are blocks without transaction data.
pub trait IHeader: Send + Sync {
    /// Returns the header hash.
    fn hash(&self) -> UInt256;

    /// Returns the block index (height).
    fn index(&self) -> u32;

    /// Returns the timestamp.
    fn timestamp(&self) -> u64;

    /// Returns the previous block hash.
    fn prev_hash(&self) -> UInt256;

    /// Returns the merkle root.
    fn merkle_root(&self) -> UInt256;
}

/// Trait for transaction data.
///
/// Provides common operations on transactions.
pub trait ITransaction: Send + Sync {
    /// Returns the transaction hash.
    fn hash(&self) -> UInt256;

    /// Returns the sender account (first signer).
    fn sender(&self) -> Option<UInt160>;

    /// Returns the system fee.
    fn system_fee(&self) -> i64;

    /// Returns the network fee.
    fn network_fee(&self) -> i64;

    /// Returns the valid until block height.
    fn valid_until_block(&self) -> u32;
}

// ============ Service Traits ============

/// Trait for blockchain query and relay operations.
///
/// This trait allows P2P actors to interact with the blockchain without
/// depending on concrete implementation types.
///
/// # Design
///
/// By using this trait:
/// 1. `LocalNode` (in neo-p2p) can query and relay blocks via trait methods
/// 2. `Blockchain` (in neo-core) implements this trait
/// 3. Tests can use mock implementations
///
/// # Associated Types
///
/// The trait uses associated types for `Block`, `Header`, and `Transaction`
/// to allow different implementations to use their own payload types.
pub trait BlockchainProvider: Send + Sync + 'static {
    /// Block type.
    type Block: IBlock;
    /// Header type.
    type Header: IHeader;
    /// Transaction type.
    type Transaction: ITransaction;

    /// Gets the current blockchain height.
    fn height(&self) -> u32;

    /// Gets a block by height.
    fn get_block(&self, height: u32) -> Option<Self::Block>;

    /// Gets a block by hash.
    fn get_block_by_hash(&self, hash: &UInt256) -> Option<Self::Block>;

    /// Gets a header by hash.
    fn get_header(&self, hash: &UInt256) -> Option<Self::Header>;

    /// Gets a header by height.
    fn get_header_by_height(&self, height: u32) -> Option<Self::Header>;

    /// Relays a block to the blockchain for validation and persistence.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the block was accepted
    /// - `Err(RelayError)` if the block was rejected
    ///
    /// # Errors
    ///
    /// Returns `RelayError` if the block validation fails, the block already exists,
    /// or the block height is invalid.
    fn relay_block(&self, block: Self::Block) -> RelayResult<()>;

    /// Relays a transaction to the memory pool.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the transaction was accepted
    /// - `Err(RelayError)` if the transaction was rejected
    ///
    /// # Errors
    ///
    /// Returns `RelayError` if the transaction is invalid or the mempool is full.
    fn relay_transaction(&self, tx: Self::Transaction) -> RelayResult<()>;

    /// Checks if a block exists in the blockchain.
    fn contains_block(&self, hash: &UInt256) -> bool;

    /// Checks if a transaction exists in the blockchain or mempool.
    fn contains_transaction(&self, hash: &UInt256) -> bool;

    /// Gets the current header hash (tip of the chain).
    fn current_header_hash(&self) -> UInt256;

    /// Gets the hash of a block at a specific height.
    fn get_block_hash(&self, height: u32) -> Option<UInt256>;
}

/// Trait for P2P peer management and message broadcasting.
///
/// This trait abstracts peer registry operations, breaking the circular
/// dependency between `LocalNode` and `PeerManagerService`.
pub trait PeerRegistry: Send + Sync + 'static {
    /// Gets the number of connected peers.
    fn connected_count(&self) -> usize;

    /// Broadcasts a message to all connected peers.
    fn broadcast(&self, message: &dyn IMessage);

    /// Broadcasts a message to all peers except the specified ones.
    fn broadcast_except(&self, message: &dyn IMessage, except: &[PeerId]);

    /// Sends a message to a specific peer.
    ///
    /// # Errors
    ///
    /// Returns `SendError` if the peer is not found, disconnected, or the send queue is full.
    fn send_to(&self, peer_id: PeerId, message: &dyn IMessage) -> SendResult<()>;

    /// Gets information about all connected peers.
    fn get_peers(&self) -> Vec<PeerInfo>;

    /// Gets information about a specific peer.
    fn get_peer(&self, peer_id: PeerId) -> Option<PeerInfo>;

    /// Checks if a peer is connected.
    fn is_connected(&self, peer_id: PeerId) -> bool {
        self.get_peer(peer_id).is_some()
    }

    /// Disconnects a peer.
    fn disconnect(&self, peer_id: PeerId);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ Mock Implementations ============

    /// Mock message for testing.
    #[derive(Debug, Clone)]
    struct MockMessage {
        command: String,
        payload: Vec<u8>,
    }

    impl MockMessage {
        fn new(command: &str, payload: Vec<u8>) -> Self {
            Self {
                command: command.to_string(),
                payload,
            }
        }
    }

    impl IMessage for MockMessage {
        fn command(&self) -> &str {
            &self.command
        }

        fn serialize(&self) -> Vec<u8> {
            self.payload.clone()
        }
    }

    /// Mock transaction for testing.
    #[derive(Debug, Clone)]
    struct MockTransaction {
        hash: UInt256,
        sender: Option<UInt160>,
        system_fee: i64,
        network_fee: i64,
        valid_until: u32,
    }

    impl ITransaction for MockTransaction {
        fn hash(&self) -> UInt256 {
            self.hash
        }
        fn sender(&self) -> Option<UInt160> {
            self.sender
        }
        fn system_fee(&self) -> i64 {
            self.system_fee
        }
        fn network_fee(&self) -> i64 {
            self.network_fee
        }
        fn valid_until_block(&self) -> u32 {
            self.valid_until
        }
    }

    /// Mock block for testing.
    #[derive(Debug, Clone)]
    struct MockBlock {
        hash: UInt256,
        index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        merkle_root: UInt256,
        tx_count: usize,
    }

    impl MockBlock {
        fn new(index: u32) -> Self {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0] = index as u8;
            Self {
                hash: UInt256::from_bytes(&hash_bytes).unwrap_or_default(),
                index,
                timestamp: 0,
                prev_hash: UInt256::zero(),
                merkle_root: UInt256::zero(),
                tx_count: 0,
            }
        }
    }

    impl IBlock for MockBlock {
        type Transaction = MockTransaction;

        fn hash(&self) -> UInt256 {
            self.hash
        }
        fn index(&self) -> u32 {
            self.index
        }
        fn timestamp(&self) -> u64 {
            self.timestamp
        }
        fn prev_hash(&self) -> UInt256 {
            self.prev_hash
        }
        fn merkle_root(&self) -> UInt256 {
            self.merkle_root
        }
        fn transaction_count(&self) -> usize {
            self.tx_count
        }
    }

    /// Mock header for testing.
    #[derive(Debug, Clone)]
    struct MockHeader {
        hash: UInt256,
        index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        merkle_root: UInt256,
    }

    impl IHeader for MockHeader {
        fn hash(&self) -> UInt256 {
            self.hash
        }
        fn index(&self) -> u32 {
            self.index
        }
        fn timestamp(&self) -> u64 {
            self.timestamp
        }
        fn prev_hash(&self) -> UInt256 {
            self.prev_hash
        }
        fn merkle_root(&self) -> UInt256 {
            self.merkle_root
        }
    }

    /// Mock blockchain provider for testing.
    struct MockBlockchain {
        height: u32,
        blocks: std::collections::HashMap<u32, MockBlock>,
    }

    impl MockBlockchain {
        fn new(height: u32) -> Self {
            let mut blocks = std::collections::HashMap::new();
            for i in 0..=height {
                blocks.insert(i, MockBlock::new(i));
            }
            Self { height, blocks }
        }
    }

    impl BlockchainProvider for MockBlockchain {
        type Block = MockBlock;
        type Header = MockHeader;
        type Transaction = MockTransaction;

        fn height(&self) -> u32 {
            self.height
        }

        fn get_block(&self, height: u32) -> Option<Self::Block> {
            self.blocks.get(&height).cloned()
        }

        fn get_block_by_hash(&self, _hash: &UInt256) -> Option<Self::Block> {
            None // Mock implementation - hash lookup not needed for tests
        }

        fn get_header(&self, _hash: &UInt256) -> Option<Self::Header> {
            None // Mock implementation - hash lookup not needed for tests
        }

        fn get_header_by_height(&self, height: u32) -> Option<Self::Header> {
            self.blocks.get(&height).map(|b| MockHeader {
                hash: b.hash,
                index: b.index,
                timestamp: b.timestamp,
                prev_hash: b.prev_hash,
                merkle_root: b.merkle_root,
            })
        }

        fn relay_block(&self, _block: Self::Block) -> RelayResult<()> {
            Ok(())
        }

        fn relay_transaction(&self, _tx: Self::Transaction) -> RelayResult<()> {
            Ok(())
        }

        fn contains_block(&self, _hash: &UInt256) -> bool {
            false
        }
        fn contains_transaction(&self, _hash: &UInt256) -> bool {
            false
        }
        fn current_header_hash(&self) -> UInt256 {
            UInt256::zero()
        }
        fn get_block_hash(&self, _height: u32) -> Option<UInt256> {
            None
        }
    }

    /// Mock peer registry for testing.
    struct MockPeerRegistry {
        peers: std::sync::Mutex<Vec<PeerInfo>>,
        broadcast_count: std::sync::atomic::AtomicUsize,
    }

    impl MockPeerRegistry {
        fn new() -> Self {
            Self {
                peers: std::sync::Mutex::new(Vec::new()),
                broadcast_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn add_peer(&self, peer: PeerInfo) {
            self.peers.lock().unwrap().push(peer);
        }

        fn broadcast_call_count(&self) -> usize {
            self.broadcast_count
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl PeerRegistry for MockPeerRegistry {
        fn connected_count(&self) -> usize {
            self.peers.lock().unwrap().len()
        }

        fn broadcast(&self, _message: &dyn IMessage) {
            self.broadcast_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        fn broadcast_except(&self, _message: &dyn IMessage, _except: &[PeerId]) {
            self.broadcast_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        fn send_to(&self, peer_id: PeerId, _message: &dyn IMessage) -> SendResult<()> {
            if self.get_peer(peer_id).is_some() {
                Ok(())
            } else {
                Err(SendError::peer_not_found(peer_id.0))
            }
        }

        fn get_peers(&self) -> Vec<PeerInfo> {
            self.peers.lock().unwrap().clone()
        }

        fn get_peer(&self, peer_id: PeerId) -> Option<PeerInfo> {
            self.peers
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == peer_id)
                .cloned()
        }

        fn disconnect(&self, peer_id: PeerId) {
            self.peers.lock().unwrap().retain(|p| p.id != peer_id);
        }
    }

    // ============ RelayError Tests ============

    #[test]
    fn test_relay_error_validation_failed() {
        let err = RelayError::validation_failed("bad merkle root");
        assert!(err.to_string().contains("block validation failed"));
        assert!(err.to_string().contains("bad merkle root"));
    }

    #[test]
    fn test_relay_error_already_exists() {
        let hash = UInt256::zero();
        let err = RelayError::already_exists(&hash);
        assert!(err.to_string().contains("block already exists"));
    }

    #[test]
    fn test_relay_error_transaction_invalid() {
        let err = RelayError::transaction_invalid("insufficient fee");
        assert!(err.to_string().contains("transaction invalid"));
    }

    #[test]
    fn test_relay_error_mempool_full() {
        let err = RelayError::mempool_full(50000, 50000);
        assert!(err.to_string().contains("memory pool full"));
    }

    #[test]
    fn test_relay_error_invalid_height() {
        let err = RelayError::invalid_height(100, 200);
        assert!(err.to_string().contains("invalid block height"));
        assert!(err.to_string().contains("expected=100"));
        assert!(err.to_string().contains("got=200"));
    }

    // ============ SendError Tests ============

    #[test]
    fn test_send_error_peer_not_found() {
        let err = SendError::peer_not_found(123);
        assert!(err.to_string().contains("peer not found"));
        assert!(err.to_string().contains("123"));
    }

    #[test]
    fn test_send_error_disconnected() {
        let err = SendError::disconnected(456);
        assert!(err.to_string().contains("peer disconnected"));
    }

    #[test]
    fn test_send_error_queue_full() {
        let err = SendError::queue_full(789);
        assert!(err.to_string().contains("send queue full"));
    }

    #[test]
    fn test_send_error_serialization() {
        let err = SendError::serialization_failed("invalid utf8");
        assert!(err.to_string().contains("serialization failed"));
    }

    // ============ PeerId Tests ============

    #[test]
    fn test_peer_id_creation() {
        let id = PeerId::new(42);
        assert_eq!(id.inner(), 42);
    }

    #[test]
    fn test_peer_id_display() {
        let id = PeerId::new(123);
        assert_eq!(format!("{}", id), "Peer(123)");
    }

    #[test]
    fn test_peer_id_equality() {
        let id1 = PeerId::new(100);
        let id2 = PeerId::new(100);
        let id3 = PeerId::new(200);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_peer_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(PeerId::new(1));
        set.insert(PeerId::new(2));
        set.insert(PeerId::new(1)); // Duplicate

        assert_eq!(set.len(), 2);
    }

    // ============ PeerInfo Tests ============

    #[test]
    fn test_peer_info_creation() {
        let info = PeerInfo::new(
            PeerId::new(1),
            "127.0.0.1:10333".to_string(),
            0,
            1234567890,
            100,
            "Neo-CLI:3.0".to_string(),
        );

        assert_eq!(info.id, PeerId::new(1));
        assert_eq!(info.address, "127.0.0.1:10333");
        assert_eq!(info.version, 0);
        assert_eq!(info.connected_at, 1234567890);
        assert_eq!(info.start_height, 100);
        assert_eq!(info.user_agent, "Neo-CLI:3.0");
    }

    // ============ IMessage Tests ============

    #[test]
    fn test_mock_message() {
        let msg = MockMessage::new("inv", vec![0x01, 0x02]);
        assert_eq!(msg.command(), "inv");
        assert_eq!(msg.serialize(), vec![0x01, 0x02]);
    }

    // ============ IBlock Tests ============

    #[test]
    fn test_mock_block() {
        let block = MockBlock::new(100);
        assert_eq!(block.index(), 100);
        assert_eq!(block.transaction_count(), 0);
    }

    // ============ BlockchainProvider Tests ============

    #[test]
    fn test_mock_blockchain_height() {
        let blockchain = MockBlockchain::new(100);
        assert_eq!(blockchain.height(), 100);
    }

    #[test]
    fn test_mock_blockchain_get_block() {
        let blockchain = MockBlockchain::new(100);

        let block = blockchain.get_block(50);
        assert!(block.is_some());
        assert_eq!(block.unwrap().index(), 50);

        let none = blockchain.get_block(200);
        assert!(none.is_none());
    }

    #[test]
    fn test_mock_blockchain_relay_block() {
        let blockchain = MockBlockchain::new(100);
        let block = MockBlock::new(101);

        let result = blockchain.relay_block(block);
        assert!(result.is_ok());
    }

    // ============ PeerRegistry Tests ============

    #[test]
    fn test_mock_peer_registry_empty() {
        let registry = MockPeerRegistry::new();
        assert_eq!(registry.connected_count(), 0);
        assert!(registry.get_peers().is_empty());
    }

    #[test]
    fn test_mock_peer_registry_add_peer() {
        let registry = MockPeerRegistry::new();
        let peer = PeerInfo::new(
            PeerId::new(1),
            "127.0.0.1:10333".to_string(),
            0,
            0,
            0,
            "".to_string(),
        );

        registry.add_peer(peer.clone());
        assert_eq!(registry.connected_count(), 1);
        assert!(registry.get_peer(PeerId::new(1)).is_some());
    }

    #[test]
    fn test_mock_peer_registry_broadcast() {
        let registry = MockPeerRegistry::new();
        let msg = MockMessage::new("ping", vec![]);

        registry.broadcast(&msg);
        registry.broadcast(&msg);

        assert_eq!(registry.broadcast_call_count(), 2);
    }

    #[test]
    fn test_mock_peer_registry_send_to() {
        let registry = MockPeerRegistry::new();
        let peer = PeerInfo::new(
            PeerId::new(1),
            "127.0.0.1:10333".to_string(),
            0,
            0,
            0,
            "".to_string(),
        );
        registry.add_peer(peer);

        let msg = MockMessage::new("ping", vec![]);

        // Send to existing peer
        let result = registry.send_to(PeerId::new(1), &msg);
        assert!(result.is_ok());

        // Send to non-existing peer
        let result = registry.send_to(PeerId::new(999), &msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_peer_registry_disconnect() {
        let registry = MockPeerRegistry::new();
        let peer = PeerInfo::new(
            PeerId::new(1),
            "127.0.0.1:10333".to_string(),
            0,
            0,
            0,
            "".to_string(),
        );

        registry.add_peer(peer);
        assert!(registry.is_connected(PeerId::new(1)));

        registry.disconnect(PeerId::new(1));
        assert!(!registry.is_connected(PeerId::new(1)));
    }

    // ============ Trait Object Tests ============

    #[test]
    fn test_message_as_trait_object() {
        fn accept_message(m: &dyn IMessage) -> &str {
            m.command()
        }

        let msg = MockMessage::new("test", vec![]);
        assert_eq!(accept_message(&msg), "test");
    }

    // ============ Send + Sync Tests ============

    #[test]
    fn test_peer_id_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PeerId>();
    }

    #[test]
    fn test_peer_info_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PeerInfo>();
    }

    #[test]
    fn test_relay_error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RelayError>();
    }

    #[test]
    fn test_send_error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SendError>();
    }

    // ============ Additional Coverage Tests ============

    #[test]
    fn test_relay_error_all_variants_eq() {
        let err1 = RelayError::ValidationFailed {
            message: "test".to_string(),
        };
        let err2 = RelayError::ValidationFailed {
            message: "test".to_string(),
        };
        let err3 = RelayError::ValidationFailed {
            message: "other".to_string(),
        };
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);

        let err4 = RelayError::AlreadyExists {
            hash: "0x123".to_string(),
        };
        let err5 = RelayError::AlreadyExists {
            hash: "0x123".to_string(),
        };
        assert_eq!(err4, err5);
        assert_ne!(err1, err4);

        let err6 = RelayError::TransactionInvalid {
            message: "bad tx".to_string(),
        };
        let err7 = RelayError::TransactionInvalid {
            message: "bad tx".to_string(),
        };
        assert_eq!(err6, err7);

        let err8 = RelayError::MempoolFull {
            current: 1000,
            max: 1000,
        };
        let err9 = RelayError::MempoolFull {
            current: 1000,
            max: 1000,
        };
        assert_eq!(err8, err9);

        let err10 = RelayError::InvalidHeight {
            expected: 100,
            got: 50,
        };
        let err11 = RelayError::InvalidHeight {
            expected: 100,
            got: 50,
        };
        assert_eq!(err10, err11);
    }

    #[test]
    fn test_send_error_all_variants_eq() {
        let err1 = SendError::PeerNotFound { id: 1 };
        let err2 = SendError::PeerNotFound { id: 1 };
        let err3 = SendError::PeerNotFound { id: 2 };
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);

        let err4 = SendError::Disconnected { id: 5 };
        let err5 = SendError::Disconnected { id: 5 };
        assert_eq!(err4, err5);
        assert_ne!(err1, err4);

        let err6 = SendError::QueueFull { id: 10 };
        let err7 = SendError::QueueFull { id: 10 };
        assert_eq!(err6, err7);

        let err8 = SendError::SerializationFailed {
            message: "bad".to_string(),
        };
        let err9 = SendError::SerializationFailed {
            message: "bad".to_string(),
        };
        assert_eq!(err8, err9);
    }

    #[test]
    fn test_relay_error_debug() {
        let err1 = RelayError::validation_failed("test");
        assert!(format!("{:?}", err1).contains("ValidationFailed"));

        let err2 = RelayError::already_exists(&UInt256::zero());
        assert!(format!("{:?}", err2).contains("AlreadyExists"));

        let err3 = RelayError::transaction_invalid("bad tx");
        assert!(format!("{:?}", err3).contains("TransactionInvalid"));

        let err4 = RelayError::mempool_full(100, 100);
        assert!(format!("{:?}", err4).contains("MempoolFull"));

        let err5 = RelayError::invalid_height(10, 20);
        assert!(format!("{:?}", err5).contains("InvalidHeight"));
    }

    #[test]
    fn test_send_error_debug() {
        let err1 = SendError::peer_not_found(1);
        assert!(format!("{:?}", err1).contains("PeerNotFound"));

        let err2 = SendError::disconnected(2);
        assert!(format!("{:?}", err2).contains("Disconnected"));

        let err3 = SendError::queue_full(3);
        assert!(format!("{:?}", err3).contains("QueueFull"));

        let err4 = SendError::serialization_failed("test");
        assert!(format!("{:?}", err4).contains("SerializationFailed"));
    }

    #[test]
    fn test_mock_transaction() {
        let tx = MockTransaction {
            hash: UInt256::zero(),
            sender: Some(UInt160::zero()),
            system_fee: 1000,
            network_fee: 500,
            valid_until: 100,
        };
        assert_eq!(tx.hash(), UInt256::zero());
        assert_eq!(tx.sender(), Some(UInt160::zero()));
        assert_eq!(tx.system_fee(), 1000);
        assert_eq!(tx.network_fee(), 500);
        assert_eq!(tx.valid_until_block(), 100);
    }

    #[test]
    fn test_mock_transaction_no_sender() {
        let tx = MockTransaction {
            hash: UInt256::zero(),
            sender: None,
            system_fee: 0,
            network_fee: 0,
            valid_until: 0,
        };
        assert!(tx.sender().is_none());
    }

    #[test]
    fn test_mock_header() {
        let header = MockHeader {
            hash: UInt256::zero(),
            index: 42,
            timestamp: 1234567890,
            prev_hash: UInt256::zero(),
            merkle_root: UInt256::zero(),
        };
        assert_eq!(header.hash(), UInt256::zero());
        assert_eq!(header.index(), 42);
        assert_eq!(header.timestamp(), 1234567890);
        assert_eq!(header.prev_hash(), UInt256::zero());
        assert_eq!(header.merkle_root(), UInt256::zero());
    }

    #[test]
    fn test_mock_block_full() {
        let block = MockBlock {
            hash: UInt256::zero(),
            index: 100,
            timestamp: 9999,
            prev_hash: UInt256::zero(),
            merkle_root: UInt256::zero(),
            tx_count: 5,
        };
        assert_eq!(block.hash(), UInt256::zero());
        assert_eq!(block.index(), 100);
        assert_eq!(block.timestamp(), 9999);
        assert_eq!(block.prev_hash(), UInt256::zero());
        assert_eq!(block.merkle_root(), UInt256::zero());
        assert_eq!(block.transaction_count(), 5);
    }

    #[test]
    fn test_blockchain_get_header_by_height() {
        let blockchain = MockBlockchain::new(50);

        let header = blockchain.get_header_by_height(25);
        assert!(header.is_some());
        let h = header.unwrap();
        assert_eq!(h.index(), 25);

        assert!(blockchain.get_header_by_height(100).is_none());
    }

    #[test]
    fn test_blockchain_relay_transaction() {
        let blockchain = MockBlockchain::new(100);
        let tx = MockTransaction {
            hash: UInt256::zero(),
            sender: None,
            system_fee: 0,
            network_fee: 0,
            valid_until: 0,
        };
        assert!(blockchain.relay_transaction(tx).is_ok());
    }

    #[test]
    fn test_blockchain_current_header_hash() {
        let blockchain = MockBlockchain::new(100);
        assert_eq!(blockchain.current_header_hash(), UInt256::zero());
    }

    #[test]
    fn test_peer_registry_broadcast_except() {
        let registry = MockPeerRegistry::new();
        let msg = MockMessage::new("test", vec![]);
        registry.broadcast_except(&msg, &[PeerId::new(1)]);
        assert_eq!(registry.broadcast_call_count(), 1);
    }

    #[test]
    fn test_peer_registry_multiple_peers() {
        let registry = MockPeerRegistry::new();
        for i in 1..=5 {
            let peer = PeerInfo::new(
                PeerId::new(i),
                format!("127.0.0.1:{}", 10333 + i),
                0,
                i * 1000,
                100,
                format!("node-{}", i),
            );
            registry.add_peer(peer);
        }

        assert_eq!(registry.connected_count(), 5);
        assert_eq!(registry.get_peers().len(), 5);

        // is_connected uses default impl
        assert!(registry.is_connected(PeerId::new(3)));
        assert!(!registry.is_connected(PeerId::new(100)));
    }

    #[test]
    fn test_result_type_aliases() {
        fn returns_relay_result() -> RelayResult<i32> {
            Ok(42)
        }

        fn returns_send_result() -> SendResult<String> {
            Ok("success".to_string())
        }

        fn returns_relay_error() -> RelayResult<i32> {
            Err(RelayError::validation_failed("test"))
        }

        fn returns_send_error() -> SendResult<String> {
            Err(SendError::peer_not_found(1))
        }

        assert_eq!(returns_relay_result().unwrap(), 42);
        assert_eq!(returns_send_result().unwrap(), "success");
        assert!(returns_relay_error().is_err());
        assert!(returns_send_error().is_err());
    }

    #[test]
    fn test_peer_id_copy_clone() {
        let id1 = PeerId::new(42);
        let id2 = id1; // Copy
        let id3 = id1;
        assert_eq!(id1, id2);
        assert_eq!(id1, id3);
    }

    #[test]
    fn test_peer_info_clone() {
        let info1 = PeerInfo::new(
            PeerId::new(1),
            "127.0.0.1:10333".to_string(),
            1,
            1000,
            100,
            "test".to_string(),
        );
        let info2 = info1.clone();
        assert_eq!(info1, info2);
    }
}
