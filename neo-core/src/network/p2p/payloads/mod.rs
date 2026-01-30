// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! P2P Payloads module matching C# Neo.Network.P2P.Payloads

/// Witness conditions for transaction verification.
pub mod conditions;

/// Address payload for peer discovery.
pub mod addr_payload;
/// Block structure and serialization.
pub mod block;
/// Conflicts transaction attribute.
pub mod conflicts;
/// Extensible payload for consensus.
pub mod extensible_payload;
/// Bloom filter add payload.
pub mod filter_add_payload;
/// Bloom filter load payload.
pub mod filter_load_payload;
/// Get block by index request payload.
pub mod get_block_by_index_payload;
/// Get blocks request payload.
pub mod get_blocks_payload;
/// Block header structure.
pub mod header;
/// Headers response payload.
pub mod headers_payload;
/// High priority transaction attribute.
pub mod high_priority_attribute;
/// Inventory interface trait.
pub mod i_inventory;
/// Inventory payload for announcements.
pub mod inv_payload;
/// Inventory type enumeration.
pub mod inventory_type;
/// Merkle block payload for SPV.
pub mod merkle_block_payload;
/// Network address with timestamp.
pub mod network_address_with_time;
/// Not valid before transaction attribute.
pub mod not_valid_before;
/// Notary assisted transaction attribute.
pub mod notary_assisted;
/// Oracle response transaction attribute.
pub mod oracle_response;
/// Oracle response code enumeration.
pub mod oracle_response_code;
/// Ping/pong payload for keepalive.
pub mod ping_payload;
/// Transaction signer structure.
pub mod signer;
/// Transaction structure and operations.
pub mod transaction;
/// Transaction attribute base.
pub mod transaction_attribute;
/// Version payload for handshake.
pub mod version_payload;
/// Witness structure for verification.
pub mod witness;
/// Witness scope flags.
pub mod witness_scope;

// Re-export commonly used types
pub use crate::ledger::VerifyResult;
// Re-export witness_rule types from root module (avoid duplicate files)
pub use crate::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};
pub use crate::IVerifiable;
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
pub use i_inventory::IInventory;
pub use inv_payload::InvPayload;
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
pub use transaction::{Transaction, HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE};
pub use transaction_attribute::TransactionAttribute;
pub use version_payload::VersionPayload;
pub use witness::Witness;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};
