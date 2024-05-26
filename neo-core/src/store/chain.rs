// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::{errors, encoding::bin::*};
use crate::block::{StatedBlock, TrimmedBlock, IndexHash};
use crate::{store::{self, *}, types::H256, tx::StatedTx};


#[derive(Debug, errors::Error)]
pub enum GetError {
    #[error("get-error: no-such-key")]
    NoSuchKey,

    #[error("get-error: bin-decode-error: {0}")]
    BinDecodeError(BinDecodeError),
}


#[derive(Debug, errors::Error)]
pub enum PutError {
    #[error("put-error: already exists")]
    AlreadyExists,
}


pub trait GetBinEncoded {
    fn get_bin_encoded<T: BinDecoder>(&self, key: &[u8]) -> Result<T, GetError>;
}

impl<Store: ReadOnlyStore> GetBinEncoded for Store
    where Store::ReadError: Into<GetError>
{
    fn get_bin_encoded<T: BinDecoder>(&self, key: &[u8]) -> Result<T, GetError> {
        let (data, _version) = self.get(key)
            .map_err(|err| err.into())?;

        let mut rb = RefBuffer::from(data.as_slice());
        BinDecoder::decode_bin(&mut rb)
            .map_err(|err| GetError::BinDecodeError(err))
    }
}


pub struct TxStore<Store: store::Store> {
    contract_id: u32,
    store: Store,
}

impl<Store: store::Store> TxStore<Store>
    where Store::ReadError: Into<GetError>,
          Store::WriteError: Into<PutError>
{
    pub fn new(contract_id: u32, store: Store) -> Self {
        Self { contract_id, store }
    }

    #[inline]
    pub fn tx_key(&self, hash: &H256) -> Vec<u8> {
        StoreKey { contract_id: self.contract_id, prefix: PREFIX_TX, key: hash }.to_bin_encoded()
    }

    pub fn get_tx(&self, hash: &H256) -> Result<StatedTx, GetError> {
        let key = self.tx_key(hash);
        let mut tx = self.store.get_bin_encoded::<StatedTx>(&key)?;

        tx.tx.calc_hash_and_size();
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

impl<Store: store::Store> BlockStore<Store>
    where Store::ReadError: Into<GetError>,
          Store::WriteError: Into<PutError>,
          <<Store as store::Store>::WriteBatch as WriteBatch>::CommitError: Into<PutError>,
{
    pub fn new(contract_id: u32, store: Store) -> Self {
        Self { contract_id, store }
    }

    pub fn get_block_hash(&self, block_index: u32) -> Result<H256, GetError> {
        let key = self.store_key(PREFIX_INDEX_TO_HASH, &big_endian::U32(block_index));
        self.store.get_bin_encoded(&key.to_bin_encoded())
    }

    #[inline]
    pub fn store_key<Key: BinEncoder>(&self, prefix: u8, key: &Key) -> Vec<u8> {
        StoreKey { contract_id: self.contract_id, prefix, key }.to_bin_encoded()
    }

    pub fn get_block_with_hash(&self, hash: &H256) -> Result<TrimmedBlock, GetError> {
        let key = self.store_key(PREFIX_BLOCK, hash);
        let mut block = self.store.get_bin_encoded::<TrimmedBlock>(&key)?;

        block.header.calc_hash();
        if !block.header.hash().eq(hash) {
            // TODO: data inconsistent
        }

        Ok(block)
    }

    pub fn get_block_with_index(&self, block_index: u32) -> Result<TrimmedBlock, GetError> {
        let hash = self.get_block_hash(block_index)?;
        self.get_block_with_hash(&hash)
    }

    pub fn put_block(&self, block: &StatedBlock) -> Result<(), PutError> {
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
        batch.commit()
            .map(|_written| ())
            .map_err(|err| err.into())
    }
}
