use crate::{UInt160, UInt256, Witness};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub previous_hash: UInt256,
    pub merkle_root: UInt256,
    pub timestamp: u64,
    pub nonce: u64,
    pub index: u32,
    pub primary_index: u8,
    pub next_consensus: UInt160,
    pub witnesses: Vec<Witness>,
}

impl BlockHeader {
    pub fn new(
        version: u32,
        previous_hash: UInt256,
        merkle_root: UInt256,
        timestamp: u64,
        nonce: u64,
        index: u32,
        primary_index: u8,
        next_consensus: UInt160,
        witnesses: Vec<Witness>,
    ) -> Self {
        Self {
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses,
        }
    }
}

impl Default for BlockHeader {
    fn default() -> Self {
        Self {
            version: 0,
            previous_hash: UInt256::zero(),
            merkle_root: UInt256::zero(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::zero(),
            witnesses: Vec::new(),
        }
    }
}
