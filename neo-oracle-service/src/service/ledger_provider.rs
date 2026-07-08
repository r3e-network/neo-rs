//! Ledger read capabilities used by the oracle service.
//!
//! Oracle response construction and signature queueing need the current ledger
//! height plus the original request transaction state. Centralizing those reads
//! keeps the service code on a provider-shaped boundary while preserving the
//! native Ledger contract's byte-for-byte storage codecs.

use neo_error::CoreResult;
use neo_native_contracts::LedgerContract;
use neo_payloads::TransactionState;
use neo_primitives::UInt256;
use neo_storage::DataCache;

/// Ledger capabilities required by oracle request processing.
pub(super) trait OracleLedgerProvider {
    /// Returns the persisted ledger height.
    fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32>;

    /// Returns the persisted transaction state for `hash`, when available.
    fn transaction_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
    ) -> CoreResult<Option<TransactionState>>;

    /// Returns the next block height used by oracle queue validity checks.
    fn next_block_height(&self, snapshot: &DataCache) -> u32 {
        self.current_index(snapshot).unwrap_or(0).saturating_add(1)
    }
}

/// Native Ledger-contract backed provider for production oracle processing.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NativeOracleLedgerProvider {
    ledger: LedgerContract,
}

impl NativeOracleLedgerProvider {
    /// Creates a provider backed by the canonical native Ledger contract codec.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            ledger: LedgerContract,
        }
    }
}

impl OracleLedgerProvider for NativeOracleLedgerProvider {
    fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.ledger.current_index(snapshot)
    }

    fn transaction_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
    ) -> CoreResult<Option<TransactionState>> {
        self.ledger.get_transaction_state(snapshot, hash)
    }
}
