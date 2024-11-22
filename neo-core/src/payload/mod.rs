// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_base::encoding::bin::*;

use neo_type::H256;
pub use {blocks::*, extensible::*, nodes::*, p2p::*};


pub mod blocks;
pub mod extensible;
pub mod nodes;
pub mod p2p;


#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum MessageFlag {
    None = 0x00,
    Compressed = 0x01,
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
    // #[bin(tag = 0x50)]
    // P2pNotaryRequest(Vec<H256>),
}

#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
pub struct Null;

#[cfg(test)]
mod test {
    use bytes::BytesMut;

    use super::*;

    #[test]
    fn test_null() {
        let null = Null;
        let mut w = BytesMut::with_capacity(128);
        null.encode_bin(&mut w);

        assert_eq!(w.len(), 0);

        let mut r = Buffer::from(w);
        let _ = Null::decode_bin(&mut r).expect("`decode_bin` should be ok");
    }
}
