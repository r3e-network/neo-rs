//
// transaction.rs - Transaction-related methods for Blockchain actor
//

use super::*;

impl Blockchain {
    pub(super) fn transaction_exists_on_chain(&self, tx: &Transaction, snapshot: &StoreCache) -> bool {
        LedgerContract::new()
            .contains_transaction(snapshot, &tx.hash())
            .unwrap_or(false)
    }

    pub(super) fn conflict_exists_on_chain(
        &self,
        tx: &Transaction,
        snapshot: &StoreCache,
        max_traceable_blocks: u32,
    ) -> bool {
        let signers: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();
        if signers.is_empty() {
            return false;
        }

        LedgerContract::new()
            .contains_conflict_hash(snapshot, &tx.hash(), &signers, max_traceable_blocks)
            .unwrap_or(false)
    }

    pub(super) fn on_new_transaction(&self, transaction: &Transaction) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let hash = transaction.hash();

        let memory_pool = context.memory_pool_handle();
        if memory_pool.lock().contains_key(&hash) {
            return VerifyResult::AlreadyInPool;
        }

        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        if ledger_contract
            .contains_transaction(&store_cache, &hash)
            .unwrap_or(false)
        {
            return VerifyResult::AlreadyExists;
        }

        let signers: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        if !signers.is_empty() {
            let settings = context.protocol_settings();
            let max_traceable = ledger_contract
                .max_traceable_blocks_snapshot(&store_cache, &settings)
                .unwrap_or(settings.max_traceable_blocks);

            if ledger_contract
                .contains_conflict_hash(&store_cache, &hash, &signers, max_traceable)
                .unwrap_or(false)
            {
                return VerifyResult::HasConflicts;
            }
        }

        let snapshot = store_cache.data_cache();
        let settings = context.protocol_settings();

        let add_result = memory_pool
            .lock()
            .try_add(transaction.clone(), snapshot, &settings);

        add_result
    }
}
