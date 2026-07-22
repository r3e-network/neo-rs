//! Transaction verification-container trait implementations.
//!
//! These adapters connect the transaction payload to witness verification and
//! hash calculation without leaving execution-layer mechanics in the transaction
//! record root.

use std::sync::Arc;

use super::Transaction;
use crate::{VerifiableContainer, VerifiableExt, Witness};
use neo_io::BinaryWriter;
use neo_primitives::{
    UInt160, Verifiable,
    error::{PrimitiveError, PrimitiveResult},
};

// The transaction wire size is provided by the canonical `Serializable::size`
// impl (see `serialization.rs`), which includes the version (1 byte), the
// script var-int length prefix, and the witnesses var-array. A previous
// inherent `Transaction::size` shadowed it with an undersized value (version
// as 4 bytes, no script prefix, witnesses omitted), corrupting `fee_per_byte`
// (mempool ordering) and the RPC `size` field — callers now use the trait.

impl Verifiable for Transaction {
    fn hash(&self) -> PrimitiveResult<neo_primitives::UInt256> {
        self.try_hash()
            .map_err(|e| PrimitiveError::invalid_data(format!("transaction hash failed: {e}")))
    }

    fn hash_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if self.serialize_unsigned(&mut writer).is_err() {
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Lightweight pre-verification that the transaction is structurally valid.
    ///
    /// Full signature and state-dependent verification is deferred to
    /// `TransactionRouter::preverify()` -> `verify_state_independent()`. This
    /// method performs basic structural checks: the version must be valid,
    /// signers must not be empty, and at least one witness must be present.
    /// Returns `false` for transactions that are structurally invalid at the
    /// protocol level.
    fn verify(&self) -> bool {
        // Reject transactions with an unknown version (C# only supports version 0).
        if self.version > 0 {
            return false;
        }
        // Every transaction must have at least one signer and at least one witness.
        if self.signers.is_empty() || self.witnesses.is_empty() {
            return false;
        }
        true
    }
}

impl VerifiableExt for Transaction {
    fn script_hashes_for_verifying(&self) -> Vec<UInt160> {
        self.signers().iter().map(|s| s.account).collect()
    }

    fn witnesses(&self) -> Vec<&Witness> {
        self.witnesses.iter().collect()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witnesses.iter_mut().collect()
    }

    /// A transaction is its own verification script container. Returning `Some`
    /// here makes witness verification install the real `Transaction` (with its
    /// signers, witness scopes/rules, and OracleResponse attributes) as the
    /// engine's script container — matching C# `Helper.VerifyWitness`, which
    /// passes the `IVerifiable` itself. Without this override the engine would
    /// fall back to a hash-only wrapper and `CheckWitness` could not see the
    /// signers during verification, wrongly rejecting contract-account,
    /// witness-rule, and OracleResponse transactions that C# accepts.
    fn as_transaction(&self) -> Option<&Transaction> {
        Some(self)
    }

    fn to_verifiable_container(&self) -> Option<Arc<VerifiableContainer>> {
        Some(Arc::new(VerifiableContainer::from(self.clone())))
    }
}
