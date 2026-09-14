//
// transaction.rs - Transaction-related methods for Blockchain actor
//

use super::*;

impl Blockchain {
    pub(super) fn transaction_exists_on_chain(
        &self,
        tx: &Transaction,
        snapshot: &StoreCache,
    ) -> bool {
        let hash = match tx.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed during on-chain lookup"
                );
                return true;
            }
        };

        LedgerContract::new()
            .contains_transaction(snapshot, &hash)
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "ledger contains_transaction failed; treating as present (fail-closed)"
                );
                true
            })
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

        let hash = match tx.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed during conflict lookup"
                );
                return true;
            }
        };

        LedgerContract::new()
            .contains_conflict_hash(snapshot, &hash, &signers, max_traceable_blocks)
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "ledger contains_conflict_hash failed; treating as conflict (fail-closed)"
                );
                true
            })
    }

    pub(super) fn on_new_transaction(&self, transaction: &Transaction) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed before mempool admission"
                );
                return VerifyResult::Invalid;
            }
        };

        let memory_pool = context.memory_pool_handle();
        if memory_pool.lock().contains_key(&hash) {
            return VerifyResult::AlreadyInPool;
        }

        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        match ledger_contract.contains_transaction(&store_cache, &hash) {
            Ok(true) => return VerifyResult::AlreadyExists,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "ledger contains_transaction failed during mempool admission"
                );
                return VerifyResult::Invalid;
            }
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

            match ledger_contract.contains_conflict_hash(
                &store_cache,
                &hash,
                &signers,
                max_traceable,
            ) {
                Ok(true) => return VerifyResult::HasConflicts,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        target: "neo",
                        error = %error,
                        "ledger contains_conflict_hash failed during mempool admission"
                    );
                    return VerifyResult::Invalid;
                }
            }
        }

        let snapshot = store_cache.data_cache();
        let settings = context.protocol_settings();

        memory_pool
            .lock()
            .try_add(transaction.clone(), snapshot, &settings)
    }
}
