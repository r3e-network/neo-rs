//! # neo-payloads
//!
//! Canonical home for the Neo P2P payload and ledger-lifecycle data types:
//! `Block`, `Header`, `Transaction`, `Signer`, witness conditions/rules,
//! transaction attributes, extensible payloads, `ApplicationExecuted`,
//! `NotifyEventArgs`, and `TransactionState`, together with their pure
//! serialization helpers and structural verification.
//!
//! Mirrors `Neo.Network.P2P.Payloads` for the heavyweight payload types
//! that historically needed a stateful verification context. The
//! ApplicationEngine-backed execution lives in `neo-execution` and native
//! contract state lookups live in `neo-native-contracts`; this crate carries
//! the data types plus the structural, state-independent checks.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends on:
//!
//! - `neo-primitives`, `neo-error`, `neo-crypto`, `neo-io`,
//!   `neo-vm-rs`, `neo-vm` (Layer 0)
//! - `neo-storage`, `neo-serialization`, `neo-manifest`,
//!   `neo-script-builder` (Layer 1)
//! - `neo-config` (Layer 1) — for `ProtocolSettings`
//! - `neo-native-contracts` (Layer 1) — for the
//!   `GasToken`/`PolicyContract`/`LedgerContract` types used by the
//!   attribute-level helpers
//!
//! Must not depend on node/runtime composition crates.
//!
//! ## Status (Stage 2)
//!
//! The data types (`Block`, `Header`, `Transaction`, `Signer`,
//! `TransactionAttribute`, `Conflicts`, `HighPriorityAttribute`,
//! `NotValidBefore`, `NotaryAssisted`, `OracleResponse`,
//! `ExtensiblePayload`, `MerkleBlockPayload`, `Inventory`,
//! `HeadersPayload`, `ApplicationExecuted`, `NotifyEventArgs`, and
//! `TransactionState`) live in this crate with their serialization impls and
//! structural `Verifiable` trait impls.
//!
//! ## Module map (C# parity)
//!
//! | C# Type | Rust module |
//! |---------|-------------|
//! | `Block` | `block` |
//! | `Header` | `header` |
//! | `Transaction` | `transaction` |
//! | `Signer` | `signer` |
//! | `WitnessCondition` / `WitnessRule` | `witness_rule` |
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
//! | `Neo.Builder` transaction helpers | `tx_builder` |
//! | `ApplicationExecuted` | `application_executed` |
//! | `NotifyEventArgs` | `notify_event_args` |
//! | `TransactionState` | `transaction_state` |

#![doc(html_root_url = "https://docs.rs/neo-payloads/0.7.2")]

// ── P2P wire payload types (relocated from neo-p2p) ───────────────────

/// Simple P2P wire payload types (serialization-only, no neo-core dependencies).
pub mod p2p_payloads;

/// Address payload for peer discovery.
pub use p2p_payloads::addr_payload;
/// Bloom filter add payload.
pub use p2p_payloads::filter_add_payload;
/// Bloom filter load payload.
pub use p2p_payloads::filter_load_payload;
/// Get block by index request payload.
pub use p2p_payloads::get_block_by_index_payload;
/// Get blocks request payload.
pub use p2p_payloads::get_blocks_payload;
/// Inventory payload for announcements.
pub use p2p_payloads::inv_payload;
/// Network address with timestamp.
pub use p2p_payloads::network_address_with_time;
/// Node capability descriptors.
pub use p2p_payloads::node_capability;
/// Ping/pong payload for keepalive.
pub use p2p_payloads::ping_payload;
/// Version payload for handshake.
pub use p2p_payloads::version_payload;

// ── Local modules: data types and structural verification ─────────────

/// Per-transaction execution record emitted when a block is processed.
pub mod application_executed;
/// Block structure and structural verification.
pub mod block;
/// Conflicts transaction attribute.
pub mod conflicts;
/// Event payloads and handler traits used by Neo plugins and services.
pub mod event_handlers;
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
/// Contract notification event arguments.
pub mod notify_event_args;
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
/// Ledger transaction state record used by `LedgerContract` storage.
pub mod transaction_state;
/// Trimmed block (header + transaction hashes) used by LedgerContract storage.
pub mod trimmed_block;
/// Fluent builders for transactions, signers, witnesses, and witness rules.
pub mod tx_builder;
/// Block validation constants (block-size / tx-count caps, merkle checks).
pub mod validation;
/// Extension of [`neo_primitives::Verifiable`] with payload-level helpers.
pub mod verifiable_ext;
/// VerifyResult re-export from `neo-primitives`.
pub mod verify_result;
/// Witness attached to verifiable payloads.
pub mod witness;
/// Witness rules and conditions used by transaction signers.
pub mod witness_rule;
/// Witness scope flags (re-exported from `neo-primitives`).
pub mod witness_scope {
    pub use neo_primitives::{InvalidWitnessScopeError, WitnessScope};
}

// ── Public re-exports ─────────────────────────────────────────────────

pub use application_executed::ApplicationExecuted;
pub use block::Block;
pub use conflicts::Conflicts;
pub use event_handlers::{
    AccountLike, CommittedHandler, CommittingHandler, MessageLike, MessageReceivedHandler,
    PluginEvent, WalletChangedHandler, WalletProvider, WitnessType,
};
pub use extensible_payload::ExtensiblePayload;
pub use header::{Header as BlockHeader, Header};
pub use headers_payload::HeadersPayload;
pub use helper::{get_sign_data, get_sign_data_vec};
pub use high_priority_attribute::HighPriorityAttribute;
pub use inventory::Inventory;
pub use merkle_block_payload::MerkleBlockPayload;
pub use not_valid_before::NotValidBefore;
pub use notary_assisted::NotaryAssisted;
pub use notify_event_args::NotifyEventArgs;
pub use oracle_response::OracleResponse;
pub use script_validation::{
    ScriptInstruction, ValidatedScript, ValidationResult, parse_script_instructions,
    validate_script, validate_strict_script,
};
pub use signer::Signer;
pub use transaction::{HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE, Transaction};
pub use transaction_attribute::TransactionAttribute;
pub use transaction_state::TransactionState;
pub use trimmed_block::TrimmedBlock;
pub use tx_builder::{
    AndConditionBuilder, OrConditionBuilder, SignerBuilder, TransactionAttributesBuilder,
    TransactionBuilder, WitnessBuilder, WitnessConditionBuilder, WitnessRuleBuilder,
};
pub use verifiable_ext::VerifiableExt;
pub use witness::Witness;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};

// Re-exports of the protocol enums.
pub use witness_rule::{
    ToStackItem, WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction,
};
pub use neo_primitives::{InventoryType, OracleResponseCode, TransactionAttributeType};
pub use verify_result::VerifyResult;
