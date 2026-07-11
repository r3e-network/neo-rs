//! Exact Ledger-row capture from a post-execution staged snapshot.

use std::collections::BTreeSet;

use neo_error::{CoreError, CoreResult};
use neo_payloads::{Block, TransactionAttribute};
use neo_static_files::{StaticRecord, StaticRow};
use neo_storage::{CacheRead, DataCache};

use crate::ledger::ledger_records::LedgerRecords;

pub(super) fn capture_block<B: CacheRead>(
    snapshot: &DataCache<B>,
    block: &Block,
) -> CoreResult<StaticRecord> {
    let block_hash = block
        .header
        .try_hash()
        .map_err(|error| CoreError::invalid_operation(format!("archive block hash: {error}")))?;
    let mut keys = BTreeSet::new();
    keys.insert(LedgerRecords::block_hash_key(block.index()));
    keys.insert(LedgerRecords::block_key(&block_hash));

    for transaction in &block.transactions {
        let transaction_hash = transaction.try_hash().map_err(|error| {
            CoreError::invalid_operation(format!("archive transaction hash: {error}"))
        })?;
        keys.insert(LedgerRecords::transaction_key(&transaction_hash));
        for conflict_hash in transaction
            .attributes()
            .iter()
            .filter_map(|attribute| match attribute {
                TransactionAttribute::Conflicts(conflict) => Some(conflict.hash),
                _ => None,
            })
        {
            keys.insert(LedgerRecords::transaction_key(&conflict_hash));
            for signer in transaction.signers() {
                keys.insert(LedgerRecords::conflict_signer_key(
                    &conflict_hash,
                    &signer.account,
                ));
            }
        }
    }

    let mut rows = Vec::with_capacity(keys.len());
    for key in keys {
        let value = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_data(format!(
                "finalized Ledger row is missing while archiving height {}: id={}, key={:02x?}",
                block.index(),
                key.id(),
                key.key()
            ))
        })?;
        rows.push(StaticRow::new(key.to_array(), value.to_value()));
    }
    Ok(StaticRecord::new(block.index(), rows))
}
