// Copyright (C) 2015-2025 The Neo Project.
//
// lib.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! # neo-payloads
//!
//! Canonical home for the Neo P2P payload data types: `Block`, `Header`,
//! `Transaction`, `Signer`, witness conditions/rules, transaction
//! attributes, and extensible payloads, together with their pure
//! serialization helpers and structural verification.
//!
//! Mirrors `Neo.Network.P2P.Payloads` for the heavyweight payload types
//! that historically needed a stateful verification context. The
//! ApplicationEngine-backed witness verification lives in
//! `neo_core::smart_contract::helper::Helper` and the native-contract
//! state lookups live in `neo_native_contracts`; this crate only carries
//! the data types plus the structural, state-independent checks.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends on:
//!
//! - `neo-primitives`, `neo-error`, `neo-crypto`, `neo-io`,
//!   `neo-vm-rs`, `neo-vm` (Layer 0)
//! - `neo-ledger-types` (Layer 1) — for `Witness`
//! - `neo-data-cache`, `neo-serialization`, `neo-block`, `neo-manifest`,
//!   `neo-redeem-script` (Layer 1)
//! - `neo-p2p` (Layer 1) — for `WitnessRule` / `WitnessCondition`
//! - `neo-config`, `neo-time` (Layer 1) — for `ProtocolSettings`,
//!   `TimeProvider`
//! - `neo-native-contracts` (Layer 1) — for the
//!   `GasToken`/`PolicyContract`/`LedgerContract` types used by the
//!   attribute-level helpers
//!
//! Must **not** depend on `neo-core` (Layer 2 runtime).
//!
//! ## Status (Stage 2)
//!
//! The data types (`Block`, `Header`, `Transaction`, `Signer`,
//! `TransactionAttribute`, `Conflicts`, `HighPriorityAttribute`,
//! `NotValidBefore`, `NotaryAssisted`, `OracleResponse`,
//! `ExtensiblePayload`, `MerkleBlockPayload`, `Inventory`,
//! `HeadersPayload`) have been moved into this crate with their
//! serialization impls and structural `Verifiable` trait impls.
//!
//! The stateful verification helpers (`script_hashes_for_verifying`,
//! full witness verification via `ApplicationEngine`,
//! native-contract-backed `verify` methods) and the
//! `TransactionVerificationContext`-aware transaction verify helpers
//! stay in `neo-core::network::p2p::payloads::{header,block,transaction}`
//! so they keep their access to the smart-contract engine.
//!
//! ## Module map (C# parity)
//!
//! | C# Type | Rust module |
//! |---------|-------------|
//! | `Block` | `block` |
//! | `Header` | `header` |
//! | `Transaction` | `transaction` |
//! | `Signer` | `signer` |
//! | `WitnessCondition` / `WitnessRule` | re-exported from `neo-p2p` |
//! | `ExtensiblePayload` | `extensible_payload` |
//! | `MerkleBlockPayload` | `merkle_block_payload` |
//! | `HeadersPayload` | `headers_payload` |
//! | `HighPriorityAttribute` | `high_priority_attribute` |
//! | `OracleResponse` | `oracle_response` |
//! | `NotValidBefore` | `not_valid_before` |
//! | `NotaryAssisted` | `notary_assisted` |
//! | `Conflicts` | `conflicts` |
//! | `TransactionAttribute` | `transaction_attribute` |
//! | `Verifiable` / `VerifiableExt` | `verifiable_ext` |
//! | `Inventory` | `inventory` |

#![doc(html_root_url = "https://docs.rs/neo-payloads/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

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

// ── Local modules: data types and structural verification ─────────────

/// Block structure and structural verification.
pub mod block;
/// Conflicts transaction attribute.
pub mod conflicts;
/// Extensible payload for consensus.
pub mod extensible_payload;
/// Block header structure and structural verification.
pub mod header;
/// Headers response payload.
pub mod headers_payload;
/// Helper utilities for signing / computing the sign-data buffer.
pub mod helper;
/// High priority transaction attribute.
pub mod high_priority_attribute;
/// Inventory interface trait.
pub mod inventory;
/// Merkle block payload for SPV.
pub mod merkle_block_payload;
/// Not valid before transaction attribute.
pub mod not_valid_before;
/// Notary assisted transaction attribute.
pub mod notary_assisted;
/// Oracle response transaction attribute.
pub mod oracle_response;
/// Strict VM script validation helpers re-exported from `neo-vm-rs`.
pub mod script_validation;
/// Transaction signer structure.
pub mod signer;
/// Transaction structure and structural verification.
pub mod transaction;
/// Transaction attribute base.
pub mod transaction_attribute;
/// Trimmed block (header + transaction hashes) used by LedgerContract storage.
pub mod trimmed_block;
/// Block validation constants (block-size / tx-count caps, merkle checks).
pub mod validation;
/// Extension of [`neo_primitives::Verifiable`] with payload-level helpers.
pub mod verifiable_ext;
/// Witness type re-export shim.
pub mod witness {
    /// Re-exported from `neo-ledger-types` so call sites that previously wrote
    /// `neo_core::network::p2p::payloads::Witness` keep resolving.
    pub use neo_ledger_types::Witness;
}
/// Witness scope flags (re-exported from `neo-primitives`).
pub mod witness_scope {
    pub use neo_primitives::{InvalidWitnessScopeError, WitnessScope};
}

// ── Public re-exports ─────────────────────────────────────────────────

pub use block::Block;
pub use conflicts::Conflicts;
pub use extensible_payload::ExtensiblePayload;
pub use header::{Header as BlockHeader, Header};
pub use headers_payload::HeadersPayload;
pub use helper::{get_sign_data, get_sign_data_vec};
pub use high_priority_attribute::HighPriorityAttribute;
pub use inventory::Inventory;
pub use merkle_block_payload::MerkleBlockPayload;
pub use not_valid_before::NotValidBefore;
pub use notary_assisted::NotaryAssisted;
pub use oracle_response::OracleResponse;
pub use script_validation::{
    parse_script_instructions, validate_script, validate_strict_script, ScriptInstruction,
    ValidatedScript, ValidationResult,
};
pub use signer::Signer;
pub use transaction::{HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE, Transaction};
pub use transaction_attribute::TransactionAttribute;
pub use trimmed_block::TrimmedBlock;
pub use verifiable_ext::VerifiableExt;
pub use witness::Witness;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};

// Re-exports of the protocol enums.
pub use neo_primitives::{
    InventoryType, OracleResponseCode, TransactionAttributeType, VerifyResult,
};
pub use neo_p2p::witness_rule::{
    ToStackItem, WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction,
};
