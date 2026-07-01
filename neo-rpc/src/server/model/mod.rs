//! # neo-rpc::server::model
//!
//! RPC request parameter models and conversion helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `address`: RPC address parameter models.
//! - `block_hash_or_index`: RPC block selector model.
//! - `contract_name_or_hash_or_id`: contract name or hash or id types and
//!   helpers.
//! - `signers_and_witnesses`: RPC signer and witness models.

/// Address parameter model.
pub mod address;
/// Block hash-or-index parameter model.
pub mod block_hash_or_index;
/// Contract name, hash, or id parameter model.
pub mod contract_name_or_hash_or_id;
/// Signers and witnesses parameter bundle.
pub mod signers_and_witnesses;

pub use address::Address;
pub use block_hash_or_index::BlockHashOrIndex;
pub use contract_name_or_hash_or_id::ContractNameOrHashOrId;
pub use signers_and_witnesses::SignersAndWitnesses;
