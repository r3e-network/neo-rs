// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Simple P2P payload types that depend only on neo-io and neo-primitives.

/// Address payload for peer discovery.
pub mod addr_payload;
/// Bloom filter add payload.
pub mod filter_add_payload;
/// Bloom filter load payload.
pub mod filter_load_payload;
/// Get block by index request payload.
pub mod get_block_by_index_payload;
/// Get blocks request payload.
pub mod get_blocks_payload;
/// Headers response payload for `GetHeaders`.
pub mod headers_payload;
/// Inventory payload for announcements.
pub mod inv_payload;
/// Merkle block payload for filtered block responses.
pub mod merkle_block_payload;
/// Network address with timestamp.
pub mod network_address_with_time;
/// Node capability descriptors.
pub mod node_capability;
/// Ping/pong payload for keepalive.
pub mod ping_payload;
/// Version payload for handshake.
pub mod version_payload;

// Re-export commonly used types
pub use addr_payload::AddrPayload;
pub use filter_add_payload::FilterAddPayload;
pub use filter_load_payload::FilterLoadPayload;
pub use get_block_by_index_payload::GetBlockByIndexPayload;
pub use get_blocks_payload::GetBlocksPayload;
pub use headers_payload::HeadersPayload;
pub use inv_payload::InvPayload;
pub use merkle_block_payload::MerkleBlockPayload;
pub use network_address_with_time::NetworkAddressWithTime;
pub use node_capability::NodeCapability;
pub use ping_payload::PingPayload;
pub use version_payload::VersionPayload;

// Re-export core protocol types from neo-protocol
pub use neo_protocol::payloads::{Block, Transaction, Witness};
pub use neo_protocol::payloads::extensible::Extensible as ExtensiblePayload;
pub use neo_protocol::execution::BlockHeader as Header;

// Re-export InventoryType from parent module for compatibility
pub use super::InventoryType;

// Re-export modules for compatibility
pub mod block {
    pub use neo_protocol::payloads::Block;
}

pub mod transaction {
    pub use neo_protocol::payloads::Transaction;
    pub const MAX_TRANSACTION_SIZE: usize = neo_protocol::payloads::transaction::MAX_TRANSACTION_SIZE;
}

pub mod extensible_payload {
    pub use neo_protocol::payloads::extensible::Extensible as ExtensiblePayload;
}

pub mod header {
    pub use neo_protocol::execution::BlockHeader as Header;
}
