//! Minimal block abstraction shared with ledger validation.
//!
//! Historically this module carried a full P2P/blockchain decoupling layer
//! (`BlockchainProvider`, `PeerRegistry`, `PeerInfo`, `PeerId`, `NetworkMessage`,
//! `HeaderLike`, `TransactionLike`, `SendError`). That layer was superseded by
//! `neo-p2p`, which owns peer identity and messaging, and by `neo-ledger`, which owns
//! block validation. Only [`BlockLike`] is still consumed: `neo-ledger` bounds block
//! validation on it so it does not need the concrete block type.

pub use errors::{RelayError, RelayResult};
pub use marker_traits::BlockLike;

/// Blockchain and relay error types.
pub mod errors;
/// Minimal marker traits used to decouple higher-level crates.
pub mod marker_traits;
