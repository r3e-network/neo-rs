use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    h256::H256,
    script::Script,
    tx::{Signer, TxAttr, Witness},
};

mod codec;

#[derive(Debug, Clone)]
pub struct Tx {
    pub version: u8,
    pub nonce: u32,
    pub valid_until_block: u32,
    pub sysfee: u64,
    pub netfee: u64,
    pub signers: Vec<Signer>,
    pub attributes: Vec<TxAttr>,
    pub script: Script,
    pub witnesses: Vec<Witness>,
}

impl Tx {
    pub fn hash(&self) -> H256 {
        super::hash::tx_hash(self)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    StateValidator = 4,
    Oracle = 8,
    NeoFSAlphabet = 16,
    P2pNotary = 32,
}
