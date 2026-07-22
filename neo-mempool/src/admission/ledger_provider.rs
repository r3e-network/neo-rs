//! Ledger read capabilities used during mempool admission.
//!
//! The mempool needs only two ledger facts while admitting transactions:
//! the current chain height and whether a `Conflicts` target is already an
//! on-chain transaction. Keeping those reads behind this small provider seam
//! prevents the admission path from constructing native Ledger contract handles
//! at each call site while preserving the exact C# storage semantics.

use neo_error::CoreResult;
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_primitives::UInt256;
use neo_storage::{CacheRead, DataCache};

/// Ledger capabilities required by transaction admission.
///
/// Production composition supplies a routed hot/cold implementation. The
/// snapshot parameter keeps this trait monomorphized while allowing the native
/// hot provider used by focused tests to share the same contract.
pub trait AdmissionLedgerProvider {
    /// Returns the persisted ledger height.
    fn current_index<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32>;

    /// Returns whether the ledger contains a full transaction record for `hash`.
    fn contains_transaction<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> CoreResult<bool>;

    /// Returns whether the ledger contains a traceable conflict record for
    /// `hash` intersecting one of `signers`.
    fn contains_conflict_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
        signers: &[neo_primitives::UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool>;
}

/// Native Ledger-contract backed provider for production admission.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct NativeAdmissionLedgerProvider {
    ledger: LedgerContract,
}

impl NativeAdmissionLedgerProvider {
    /// Creates a provider backed by the canonical native Ledger contract codec.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            ledger: LedgerContract,
        }
    }
}

impl AdmissionLedgerProvider for NativeAdmissionLedgerProvider {
    fn current_index<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.ledger.current_index(snapshot)
    }

    fn contains_transaction<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> CoreResult<bool> {
        self.ledger.contains_transaction(snapshot, hash)
    }

    fn contains_conflict_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
        signers: &[neo_primitives::UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        self.ledger
            .contains_conflict_hash(snapshot, hash, signers, max_traceable_blocks)
    }
}
