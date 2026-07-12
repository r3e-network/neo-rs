//! # neo-payloads
//!
//! Protocol payload records for blocks, transactions, witnesses, and P2P
//! messages.
//!
//! ## Boundary
//!
//! This protocol crate owns payload records and validation helpers and must not
//! perform IO, storage commits, or service orchestration.
//!
//! ## Contents
//!
//! - `p2p_payloads`: P2P payload records and network inventory message types.
//! - `execution`: Execution payload records and VM-result domain types.
//! - `ledger`: Ledger caches, lookup context, and persisted record helpers used
//!   by block import.
//! - `protocol`: Protocol enums, versioned records, and chain-level domain
//!   constants.
//! - `signing`: Witness, signer, and signature validation helpers.
//! - `validation`: Validation routines and typed verdicts for protocol data.
//! - `transaction`: Transaction body, signer, witness, and fee records.
//! - `transaction_attribute`: Transaction attribute records and validation
//!   helpers.
//! - `tx_builder`: Transaction builder helpers for constructing Neo payloads.

#![doc(html_root_url = "https://docs.rs/neo-payloads/0.10.0")]

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

/// Execution-result and notification payloads emitted by block processing.
pub mod execution;
/// Ledger payloads such as blocks, headers, and transaction state records.
pub mod ledger;
/// P2P protocol payload traits and consensus extension payloads.
pub mod protocol;
/// Transaction signing, witness, and verification helper types.
pub mod signing;
/// Structural validation constants and VM script validation helpers.
pub mod validation;

/// Transaction structure and structural verification.
pub mod transaction;
/// Transaction attribute base.
#[path = "transaction_attribute/mod.rs"]
pub mod transaction_attribute;
/// Fluent builders for transactions, signers, witnesses, and witness rules.
pub mod tx_builder;
/// Witness scope flags (re-exported from `neo-primitives`).
pub mod witness_scope {
    pub use neo_primitives::{InvalidWitnessScopeError, WitnessScope};
}

pub use execution::{application_executed, event_handlers, log_event_args, notify_event_args};
pub use ledger::{
    block, header, headers_payload, merkle_block_payload, transaction_state, trimmed_block,
};
pub use protocol::{extensible_payload, inventory};
pub use signing::{helper, signer, verifiable_container, verifiable_ext, witness, witness_rule};
pub use transaction_attribute::{
    conflicts, high_priority_attribute, not_valid_before, notary_assisted, oracle_response,
};
pub use validation::{script_validation, verify_result};

// ── Public re-exports ─────────────────────────────────────────────────

pub use application_executed::ApplicationExecuted;
pub use block::Block;
pub use conflicts::Conflicts;
pub use event_handlers::{
    CommittedHandler, CommittingHandler, FinalizedHandler, PluginEvent, WalletChangedHandler,
    WitnessType,
};
pub use extensible_payload::ExtensiblePayload;
pub use header::{Header as BlockHeader, Header};
pub use headers_payload::HeadersPayload;
pub use helper::{get_sign_data, get_sign_data_vec};
pub use high_priority_attribute::HighPriorityAttribute;
pub use inventory::Inventory;
pub use log_event_args::LogEventArgs;
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
pub use verifiable_container::{VerifiableContainer, VerifiableHashContainer};
pub use verifiable_ext::VerifiableExt;
pub use witness::Witness;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};

// Re-exports of the protocol enums.
pub use neo_primitives::{InventoryType, OracleResponseCode, TransactionAttributeType};
pub use verify_result::VerifyResult;
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
