//! Store-backed block source for local ledger mode.

use std::sync::Arc;

/// Read-only ledger view that serves peers' block requests
/// ([`neo_network::BlockSource`]) by reconstructing a full block from the
/// persistent store: `index -> hash -> TrimmedBlock -> transactions`
/// (the C# `NativeContract.Ledger.GetBlock(snapshot, index)` path).
pub(in crate::node) struct LedgerBlockSource {
    snapshot: Arc<neo_storage::persistence::DataCache>,
    /// Blockchain relay cache for accepted extensible payloads (dBFT and
    /// state-service messages).
    ledger: Arc<neo_blockchain::LedgerContext>,
    /// The shared mempool, so `Inv`/`Mempool` gossip can answer for
    /// unconfirmed transactions (which are not yet in the ledger snapshot).
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl LedgerBlockSource {
    pub(in crate::node) fn new(
        snapshot: Arc<neo_storage::persistence::DataCache>,
        ledger: Arc<neo_blockchain::LedgerContext>,
        mempool: Arc<neo_mempool::MemoryPool>,
    ) -> Self {
        Self {
            snapshot,
            ledger,
            mempool,
        }
    }

    /// Reconstructs the full block stored under `hash`: header plus the
    /// transactions referenced by its `TrimmedBlock`.
    fn full_block(
        &self,
        ledger: &neo_native_contracts::LedgerContract,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Block> {
        let trimmed = ledger.get_trimmed_block(&self.snapshot, hash).ok()??;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let state = ledger
                .get_transaction_state(&self.snapshot, tx_hash)
                .ok()??;
            transactions.push(state.transaction?);
        }
        Some(neo_payloads::Block::from_parts(
            trimmed.header,
            transactions,
        ))
    }
}

impl neo_network::BlockSource for LedgerBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        self.full_block(&ledger, &hash)
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        let trimmed = ledger.get_trimmed_block(&self.snapshot, &hash).ok()??;
        Some(trimmed.header)
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&self.snapshot, index)
            .ok()
            .flatten()
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        self.full_block(&neo_native_contracts::LedgerContract::new(), hash)
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        neo_native_contracts::LedgerContract::new()
            .get_trimmed_block(&self.snapshot, hash)
            .ok()
            .flatten()
            .map(|trimmed| trimmed.header.index())
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        // Serve unconfirmed transactions from the mempool first (C# `GetData`
        // serves `MemoryPool` entries), then fall back to the ledger.
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        neo_native_contracts::LedgerContract::new()
            .get_transaction_state(&self.snapshot, hash)
            .ok()?
            .and_then(|state| state.transaction)
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        self.mempool.contains(hash)
            || neo_native_contracts::LedgerContract::new()
                .get_transaction_state(&self.snapshot, hash)
                .ok()
                .flatten()
                .is_some()
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        self.mempool
            .verified_snapshot()
            .iter()
            .map(|item| item.hash())
            .collect()
    }
}
