use super::errors::*;
use super::marker_traits::*;
use super::peer::*;
use crate::UInt256;

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
    type Block: BlockLike;
    /// Header type.
    type Header: HeaderLike;
    /// Transaction type.
    type Transaction: TransactionLike;

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
    fn broadcast(&self, message: &dyn NetworkMessage);

    /// Broadcasts a message to all peers except the specified ones.
    fn broadcast_except(&self, message: &dyn NetworkMessage, except: &[PeerId]);

    /// Sends a message to a specific peer.
    ///
    /// # Errors
    ///
    /// Returns `SendError` if the peer is not found, disconnected, or the send queue is full.
    fn send_to(&self, peer_id: PeerId, message: &dyn NetworkMessage) -> SendResult<()>;

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
