// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_base::encoding::bin::*;

use crate::block::Header;
use crate::types::{Bytes, H256};

pub const MAX_HEADERS_ALLOWED: usize = 2000;
pub const MAX_HASH_COUNT: usize = 500;

/// i.e. GetBlockByIndex
#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct BlockIndexRange {
    pub start_index: u32,
    pub count: u16,
}

/// i.e. GetBlocks
#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct BlockHashRange {
    pub start_hash: H256,
    pub count: u16,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct Headers {
    pub headers: Vec<Header>,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct MerkleBlock {
    pub header: Header,
    pub tx_count: u32,
    pub hashes: Vec<H256>,
    pub flags: Bytes,
}
