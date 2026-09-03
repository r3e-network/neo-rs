// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms, with or without
// modifications are permitted.

//! P2P Payloads module matching C# Neo.Network.P2P.Payloads
//!
//! Simple payload types are defined in `neo_p2p::payloads` and re-exported here.
//! Payloads with neo-core dependencies (Header, Block, etc.) remain here.

// ── Re-exports from neo-p2p for simple payload types ──────────────────

/// Address payload for peer discovery.
pub use neo_p2p::payloads::addr_payload;
/// Bloom filter add payload.
pub use neo_p2p::payloads::filter_add_payload;
/// Bloom filter load payload.
pub use neo_p2p::payloads::filter_load_payload;
/// Get block by index request payload.
pub use neo_p2p::payloads::get_block_by_index_payload;
/// Get blocks request payload.
pub use neo_p2p::payloads::get_blocks_payload;
/// Inventory payload for announcements.
pub use neo_p2p::payloads::inv_payload;
/// Node capability descriptors.
pub use neo_p2p::payloads::node_capability;
/// Network address with timestamp.
pub use neo_p2p::payloads::network_address_with_time;
/// Ping/pong payload for keepalive.
pub use neo_p2p::payloads::ping_payload;
/// Version payload for handshake.
pub use neo_p2p::payloads::version_payload;

// ── Local modules (neo-core dependencies) ─────────────────────────────

/// Witness conditions for transaction verification.
pub mod conditions;
/// Block structure and serialization.
pub mod block;
/// Conflicts transaction attribute.
pub mod conflicts;
/// Extensible payload for consensus.
pub mod extensible_payload;
/// Block header structure.
pub mod header;
/// Headers response payload.
pub mod headers_payload;
/// High priority transaction attribute.
pub mod high_priority_attribute;
/// Inventory interface trait.
pub mod inventory;
/// Inventory type enumeration.
pub mod inventory_type {
    pub use neo_primitives::InventoryType;
}
/// Merkle block payload for SPV.
pub mod merkle_block_payload;
/// Not valid before transaction attribute.
pub mod not_valid_before;
/// Notary assisted transaction attribute.
pub mod notary_assisted;
/// Oracle response transaction attribute.
pub mod oracle_response;
/// Oracle response code enumeration.
pub mod oracle_response_code {
    pub use neo_primitives::OracleResponseCode;
}
/// Transaction signer structure.
pub mod signer;
/// Transaction structure and operations.
pub mod transaction;
/// Transaction attribute base.
pub mod transaction_attribute;
/// Witness structure for verification.
pub mod witness {
    pub use crate::witness::Witness;
}
/// Witness scope flags.
pub mod witness_scope {
    pub use neo_primitives::{InvalidWitnessScopeError, WitnessScope};
}

// Re-export commonly used types
pub use crate::ledger::VerifyResult;
// Re-export witness_rule types from root module (avoid duplicate files)
pub use crate::Verifiable;
pub use crate::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};
pub use addr_payload::AddrPayload;
pub use block::Block;
pub use conflicts::Conflicts;
pub use extensible_payload::ExtensiblePayload;
pub use filter_add_payload::FilterAddPayload;
pub use filter_load_payload::FilterLoadPayload;
pub use get_block_by_index_payload::GetBlockByIndexPayload;
pub use get_blocks_payload::GetBlocksPayload;
pub use header::Header;
pub use headers_payload::HeadersPayload;
pub use high_priority_attribute::HighPriorityAttribute;
pub use inv_payload::InvPayload;
pub use inventory::Inventory;
pub use inventory_type::InventoryType;
pub use merkle_block_payload::MerkleBlockPayload;
pub use neo_primitives::TransactionAttributeType;
pub use network_address_with_time::NetworkAddressWithTime;
pub use not_valid_before::NotValidBefore;
pub use notary_assisted::NotaryAssisted;
pub use oracle_response::OracleResponse;
pub use oracle_response_code::OracleResponseCode;
pub use ping_payload::PingPayload;
pub use signer::Signer;
pub use transaction::{HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE, Transaction};
pub use transaction_attribute::TransactionAttribute;
pub use version_payload::VersionPayload;
pub use witness::Witness;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};
