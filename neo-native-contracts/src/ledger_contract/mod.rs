//! # neo-native-contracts::ledger_contract
//!
//! Native Ledger contract storage and query behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: read-only native method handlers for ledger queries.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `queries`: read-provider helpers for snapshots and trace windows.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `wire`: Wire encoders, decoders, and deterministic network framing
//!   helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::LEDGER_CONTRACT_HASH;
use neo_error::CoreResult;
use neo_execution::{NativeContract, NativeMethod};
use neo_payloads::TrimmedBlock;
use neo_primitives::UInt256;

mod invoke;
mod metadata;
mod queries;
/// LedgerContract storage prefixes and key builders shared with the
/// blockchain persist pipeline.
pub mod storage;
mod wire;

native_contract_handle!(
    /// Static accessor for the LedgerContract native contract.
    pub struct LedgerContract {
        id: -4,
        contract_name: "LedgerContract",
        hash: LEDGER_CONTRACT_HASH,
    }
);

impl NativeContract for LedgerContract {
    native_contract_identity!(LedgerContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::LEDGER_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    native_contract_dispatch!(metadata::LEDGER_CONTRACT_METHOD_BINDINGS);

    fn transaction_state(
        &self,
        snapshot: &neo_storage::DataCache,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        self.get_transaction_state(snapshot, tx_hash)
    }

    fn trimmed_block(
        &self,
        snapshot: &neo_storage::DataCache,
        block_hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        self.get_trimmed_block(snapshot, block_hash)
    }
}

#[cfg(test)]
#[path = "../tests/ledger_contract/mod.rs"]
mod tests;
