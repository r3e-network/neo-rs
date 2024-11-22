// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::encoding::bin::*;

use crate::block::{IndexHash, StatedBlock, TrimmedBlock};
use crate::store::{self, *};
use crate::tx::StatedTx;
use neo_type::H256;

pub const PREFIX_BLOCK: u8 = 5;
pub const PREFIX_INDEX_TO_HASH: u8 = 9;
pub const PREFIX_TX: u8 = 11;
pub const PREFIX_CURRENT_BLOCK: u8 = 12;

pub struct TxStore<Store: store::Store> {
    contract_id: u32,
    store: Store,
}

impl<Store: store::Store> TxStore<Store> {
    pub fn new(contract_id: u32, store: Store) -> Self { Self { contract_id, store } }

    #[inline]
    fn tx_key(&self, hash: &H256) -> Vec<u8> {
        StoreKey::new(self.contract_id, PREFIX_TX, hash).to_bin_encoded()
    }

    pub fn get_tx(&self, hash: &H256) -> Result<StatedTx, BinReadError> {
        let key = self.tx_key(hash);
        let mut tx = self.store.get_bin_encoded::<StatedTx>(&key)?;

        tx.tx.recalc_hash();
        if !tx.tx.hash().eq(hash) {
            // TODO: data inconsistent
        }

        Ok(tx)
    }
}

pub struct BlockStore<Store: store::Store> {
    contract_id: u32,
    store: Store,
}

impl<Store: store::Store> BlockStore<Store> {
    pub fn new(contract_id: u32, store: Store) -> Self { Self { contract_id, store } }

    pub fn get_block_hash(&self, block_index: u32) -> Result<H256, BinReadError> {
        let key = self.store_key(PREFIX_INDEX_TO_HASH, &big_endian::U32(block_index));
        self.store.get_bin_encoded(&key.to_bin_encoded())
    }

    #[inline]
    fn store_key<Key: BinEncoder + Debug>(&self, prefix: u8, key: &Key) -> Vec<u8> {
        StoreKey::new(self.contract_id, prefix, key).to_bin_encoded()
    }

    pub fn get_block_with_hash(&self, hash: &H256) -> Result<TrimmedBlock, BinReadError> {
        let key = self.store_key(PREFIX_BLOCK, hash);
        let mut block = self.store.get_bin_encoded::<TrimmedBlock>(&key)?;

        block.header.calc_hash();
        if !block.header.hash().eq(hash) {
            // TODO: data inconsistent
        }

        Ok(block)
    }

    pub fn get_block_with_index(&self, block_index: u32) -> Result<TrimmedBlock, BinReadError> {
        let hash = self.get_block_hash(block_index)?;
        self.get_block_with_hash(&hash)
    }

    pub fn put_block(&self, block: &StatedBlock) -> Result<(), BinWriteError> {
        let mut batch = self.store.write_batch();

        let block_index = block.header.index;
        let hash = block.hash();
        for tx in block.txs() {
            let encoded_tx = tx.to_bin_encoded();
            for conflict in tx.tx.conflicts() {
                batch.add_put(
                    self.store_key(PREFIX_TX, &conflict.hash),
                    encoded_tx.clone(),
                    &WriteOptions::with_always(),
                );

                for signer in tx.tx.signers() {
                    batch.add_put(
                        self.store_key(PREFIX_TX, &(&hash, signer)),
                        encoded_tx.clone(),
                        &WriteOptions::with_always(),
                    );
                }
            }

            // tx in this block
            batch.add_put(
                self.store_key(PREFIX_TX, &tx.hash()),
                encoded_tx,
                &WriteOptions::with_always(),
            );
        }

        // block index -> block hash
        batch.add_put(
            self.store_key(PREFIX_INDEX_TO_HASH, &big_endian::U32(block_index)),
            hash.to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        // block itself
        batch.add_put(
            self.store_key(PREFIX_BLOCK, &hash),
            block.to_trimmed_block().to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        // set index & hash of current block
        batch.add_put(
            self.store_key(PREFIX_CURRENT_BLOCK, &()),
            IndexHash { hash, index: block_index }.to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        // Commit all, must be all succeed or all failed
        batch.commit().map(|_written| ()).map_err(|err| match err {
            CommitError::Conflicted => BinWriteError::AlreadyExists,
        })
    }
}
