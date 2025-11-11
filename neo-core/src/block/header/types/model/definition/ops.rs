use alloc::vec::Vec;

use crate::{
    block::merkle::compute_merkle_root,
    h160::H160,
    h256::H256,
    tx::{Tx, Witness},
};

use super::Header;

impl Header {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: u32,
        prev_hash: H256,
        merkle_root: H256,
        unix_milli: u64,
        nonce: u64,
        index: u32,
        primary: u8,
        next_consensus: H160,
        witnesses: Vec<Witness>,
    ) -> Self {
        Self {
            hash: None,
            version,
            prev_hash,
            merkle_root,
            unix_milli,
            nonce,
            index,
            primary,
            next_consensus,
            witnesses,
            state_root_enabled: false,
            prev_state_root: None,
        }
    }

    pub fn state_root_enabled(&self) -> bool {
        self.state_root_enabled
    }

    pub fn set_state_root(&mut self, root: Option<H256>) {
        self.state_root_enabled = root.is_some();
        self.prev_state_root = root;
        self.hash = None;
    }

    pub fn verify_merkle_root(&self, txs: &[Tx]) -> bool {
        let hashes: Vec<H256> = txs.iter().map(|tx| tx.hash()).collect();
        self.merkle_root == compute_merkle_root(&hashes)
    }
}
