// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use bytes::BytesMut;

use neo_base::{errors, encoding::bin::*};
use crate::{
    store::{self, dal::DalStore},
    types::{H256, H256_SIZE},
    tx::{Tx, StatedTx},
};


pub const PREFIX_BLOCK: u8 = 5;
pub const PREFIX_INDEX_TO_HASH: u8 = 9;
pub const PREFIX_TX: u8 = 11;
pub const PREFIX_CURRENT_BLOCK: u8 = 12;


#[derive(Debug, errors::Error)]
pub enum GetError {
    #[error("get-error: no-such-key")]
    NoSuchKey,

    #[error("get-error: bin-decode-error: {0}")]
    BinDecodeError(BinDecodeError),

    // #[error("get-error: unexpected error: `{0}`")]
    //  Unexpected(String),
}


#[derive(Debug, errors::Error)]
pub enum PutError {
    #[error("put-error: already exists")]
    AlreadyExists,

    // #[error("put-error: unexpected error: `{0}`")]
    // Unexpected(String),
}


pub struct TxStore<Store: store::Store> {
    contract_id: u32,
    dal: DalStore<Store>,
}

impl<Store: store::Store> TxStore<Store>
    where Store::ReadError: Into<GetError>,
          Store::WriteError: Into<PutError>
{
    pub fn new(contract_id: u32, dal: DalStore<Store>) -> Self {
        Self { contract_id, dal }
    }

    pub fn get_tx(&self, hash: &H256) -> Result<Tx, GetError> {
        let mut buf = BytesMut::with_capacity(1 + H256_SIZE);
        PREFIX_TX.encode_bin(&mut buf);
        hash.encode_bin(&mut buf);

        let (data, _version) = self.dal.get(self.contract_id, buf.as_ref())
            .map_err(|err| err.into())?;

        let mut rb = RefBuffer::from(data.as_slice());
        let stated_tx: StatedTx = BinDecoder::decode_bin(&mut rb)
            .map_err(|err| GetError::BinDecodeError(err))?;

        Ok(stated_tx.tx)
    }

    //pub fn put_tx(&mut self, tx: &Tx) -> Result<Tx, PutError> {
    //
    //}
}


pub struct BlockStore<Store: store::Store> {
    contract_id: u32,
    dal: DalStore<Store>,
}

impl<Store: store::Store> BlockStore<Store>
    where Store::ReadError: Into<GetError>,
          Store::WriteError: Into<PutError>
{
    pub fn new(contract_id: u32, dal: DalStore<Store>) -> Self {
        Self { contract_id, dal }
    }

    // pub fn get_header_with_hash(&self, hash: &H256) -> Result<Header, GetError> {
    //     //
    // }
    //
    // pub fn get_header_with_index(&self, block_index: u32) -> Result<Header, GetError> {
    //     //
    // }
    //
    // pub fn put_header(&mut self, block: &Block) -> Result<(), PutError> {
    //     //
    // }
}