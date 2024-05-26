// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub mod blocks;
pub mod nodes;
pub mod extensible;
pub mod pool;


use alloc::vec::Vec;

use neo_base::encoding::bin::*;
use crate::types::H256;

pub use {blocks::*, extensible::*, nodes::*};


#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum MessageFlag {
    None = 0x00,
    Compressed = 0x01,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub enum MessageCommand {
    Version = 0x00,
    Verack = 0x01,

    GetAddr = 0x10,
    Addr = 0x11,
    Ping = 0x18,
    Pong = 0x19,

    GetHeaders = 0x20,
    Headers = 0x21,
    GetBlocks = 0x24,

    /// i.e. MemPool
    TxPool = 0x25,

    Inventory = 0x27,
    GetData = 0x28,
    GetBlockByIndex = 0x29,
    NotFound = 0x2a,
    Tx = 0x2b,
    Block = 0x2c,
    Extensible = 0x2e,
    Reject = 0x2f,

    FilterLoad = 0x30,
    FilterAdd = 0x31,
    FilterClear = 0x32,
    MerkleBlock = 0x38,

    Alert = 0x40,

    P2PNotaryRequest = 0x50,
    // GetMPTData = 0x51,
    // MPTData = 0x52,
}


/// i.e InvPayload
#[derive(Debug, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum Inventory {
    #[bin(tag = 0x2b)]
    Tx(Vec<H256>),

    #[bin(tag = 0x2c)]
    Block(Vec<H256>),

    #[bin(tag = 0x2e)]
    Extensible(Vec<H256>),

    #[bin(tag = 0x50)]
    P2PNotaryRequest(Vec<H256>),
}


#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
pub struct Null;


#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_null() {
        let null = Null;
        let mut w = BytesMut::with_capacity(128);
        null.encode_bin(&mut w);

        assert_eq!(w.len(), 0);

        let mut r = Buffer::from(w);
        let _ = Null::decode_bin(&mut r)
            .expect("`decode_bin` should be ok");
    }
}