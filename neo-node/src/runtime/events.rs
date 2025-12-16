//! Runtime events emitted by the node.

/// Events emitted by the runtime
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// Node started
    Started,
    /// Node stopping
    Stopping,
    /// New block applied
    BlockApplied { height: u32, hash: [u8; 32] },
    /// New transaction added to mempool
    TransactionAdded { hash: [u8; 32] },
    /// Peer connected
    PeerConnected { address: String },
    /// Peer disconnected
    PeerDisconnected { address: String },
    /// Consensus state changed
    ConsensusStateChanged { view: u8, block_index: u32 },
}
