// Copyright (C) 2015-2025 The Neo Project.
//
// transaction/mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Transaction payload.
//!
//! The canonical [`Transaction`] type is owned by `neo-protocol` and
//! re-exported here for backward compatibility (R2 leaf-type unification).
//! All structural API — getters/setters, hashing (`hash`/`try_hash`), wire
//! [`neo_io_crate::Serializable`], `to_json`, `Default`, `std::hash::Hash`,
//! `Inventory`, and `neo_primitives::Verifiable` — is provided by
//! `neo-protocol`.
//!
//! neo-core keeps only the pieces that depend on the snapshot / VM layer:
//! the [`TransactionExt`] verification helpers, the VM stack projection free
//! functions ([`transaction_to_stack_item`] / [`transaction_to_stack_value`]),
//! and the local [`crate::witness::WitnessData`] / [`crate::VerifiableExt`]
//! trait implementations.

/// Canonical transaction payload, owned by `neo-protocol`.
pub use neo_protocol::payloads::transaction::Transaction;
/// Canonical transaction size / attribute limits, owned by `neo-protocol`.
pub use neo_protocol::payloads::transaction::{
    HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE,
};

// Include implementation files (neo-core specific: snapshot / VM layer only).
mod ledger_adapter;
mod stack_projection;
mod traits;
mod verification;
mod witness_verification;

pub use stack_projection::{transaction_to_stack_item, transaction_to_stack_value};
pub use verification::{
    TransactionExt, verify_transaction, verify_transaction_state_dependent,
    verify_transaction_state_dependent_at_height, verify_transaction_state_independent,
};
