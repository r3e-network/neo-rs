//! Block and transaction materialization before indexing.
//!
//! This module converts a canonical block into immutable index records while
//! validating duplicate transactions and bounded transaction positions.

use std::collections::HashSet;

use neo_payloads::Block;

use crate::error::{IndexerError, IndexerResult};
use crate::model::{BlockIndexRecord, TransactionIndexRecord};

#[derive(Debug)]
pub(crate) struct PreparedBlock {
    pub(super) block: BlockIndexRecord,
    pub(super) transactions: Vec<TransactionIndexRecord>,
}

pub(super) fn prepare_block(block: &Block) -> IndexerResult<PreparedBlock> {
    let block_hash = block
        .try_hash()
        .map_err(|source| IndexerError::BlockHash { source })?;
    let transaction_count =
        u32::try_from(block.transactions.len()).map_err(|_| IndexerError::TooManyTransactions {
            count: block.transactions.len(),
        })?;

    let mut transactions = Vec::with_capacity(block.transactions.len());
    let mut seen_transactions = HashSet::with_capacity(block.transactions.len());
    for (position, transaction) in block.transactions.iter().enumerate() {
        let transaction_index =
            u32::try_from(position).map_err(|_| IndexerError::TooManyTransactions {
                count: block.transactions.len(),
            })?;
        let hash = transaction
            .try_hash()
            .map_err(|source| IndexerError::TransactionHash {
                index: transaction_index,
                source,
            })?;
        if !seen_transactions.insert(hash) {
            return Err(IndexerError::DuplicateTransaction { hash });
        }

        let mut seen_accounts = HashSet::new();
        let mut signers = Vec::new();
        for signer in transaction.signers() {
            if seen_accounts.insert(signer.account) {
                signers.push(signer.account);
            }
        }

        transactions.push(TransactionIndexRecord {
            hash,
            block_hash,
            block_height: block.index(),
            transaction_index,
            signers,
        });
    }

    Ok(PreparedBlock {
        block: BlockIndexRecord {
            hash: block_hash,
            height: block.index(),
            timestamp: block.timestamp(),
            transaction_count,
        },
        transactions,
    })
}
