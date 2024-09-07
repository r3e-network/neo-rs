// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::vec::Vec;

use neo_base::{errors, encoding::bin::*};
use crate::{block::Block, payload::*, tx::Tx};


const MIN_COMPRESS_SIZE: usize = 1024;

pub const MAX_COUNT_TO_SEND: usize = 200;


#[derive(Debug, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum P2pMessage {
    #[bin(tag = 0x00)]
    Version(Version),

    #[bin(tag = 0x01)]
    VersionAck,

    #[bin(tag = 0x10)]
    GetAddress,

    #[bin(tag = 0x11)]
    Address(NodeList),

    #[bin(tag = 0x18)]
    Ping(Ping),

    #[bin(tag = 0x19)]
    Pong(Pong),

    #[bin(tag = 0x20)]
    GetHeaders(BlockIndexRange),

    #[bin(tag = 0x21)]
    Headers(Headers),

    #[bin(tag = 0x24)]
    GetBlocks(BlockHashRange),

    #[bin(tag = 0x25)]
    TxPool,

    #[bin(tag = 0x27)]
    Inventory(Inventory),

    #[bin(tag = 0x28)]
    GetData(Inventory),

    #[bin(tag = 0x29)]
    GetBlockByIndex(BlockIndexRange),

    #[bin(tag = 0x2a)]
    NotFound(Inventory),

    #[bin(tag = 0x2b)]
    Tx(Tx),

    #[bin(tag = 0x2c)]
    Block(Block),

    #[bin(tag = 0x2e)]
    Extensible(Extensible),

    // Sent to reject an inventory.
    #[bin(tag = 0x2f)]
    Reject,

    #[bin(tag = 0x30)]
    FilterLoad(FilterLoad),

    #[bin(tag = 0x31)]
    FilterAdd(FilterAdd),

    #[bin(tag = 0x32)]
    FilterClear,

    #[bin(tag = 0x38)]
    MerkleBlock(MerkleBlock),

    #[bin(tag = 0x40)]
    Alert,
}


impl P2pMessage {
    #[inline]
    pub fn is_single(&self) -> bool {
        use P2pMessage::*;
        matches!(self, GetAddress | Address(_) | Ping(_) | Pong(_) | GetHeaders(_) | GetBlocks(_) | TxPool)
    }

    #[inline]
    pub fn is_high_priority(&self) -> bool {
        use P2pMessage::*;
        matches!(self, Alert | Extensible(_) | FilterAdd(_) | FilterClear | FilterLoad(_) | GetAddress | TxPool)
    }

    #[inline]
    pub fn can_compress(&self) -> bool {
        use P2pMessage::*;
        matches!(self, Address(_) | Block(_) | Extensible(_) | Tx(_) | Headers(_) | MerkleBlock(_) | FilterLoad(_) | FilterAdd(_))
    }
}


#[inline]
pub fn can_compress(typ: u8) -> bool {
    matches!(typ, 0x2c | 0x2e | 0x2b | 0x21 | 0x11 | 0x38 | 0x30 | 0x31)
}


#[derive(Debug, Clone, errors::Error)]
pub enum Lz4CompressError {
    #[error("lz4-compress: compress is unworthy")]
    Unworthy,
}

pub type Lz4DecompressError = lz4_flex::block::DecompressError;

pub trait Lz4Compress {
    fn lz4_compress(&self) -> Result<Vec<u8>, Lz4CompressError>;
}

pub trait Lz4Decompress: Sized {
    fn lz4_decompress(data: &[u8]) -> Result<Self, Lz4DecompressError>;
}


impl<T: AsRef<[u8]>> Lz4Compress for T {
    #[inline]
    fn lz4_compress(&self) -> Result<Vec<u8>, Lz4CompressError> {
        let data = self.as_ref();
        if data.len() < MIN_COMPRESS_SIZE {
            return Err(Lz4CompressError::Unworthy);
        }

        let compressed = lz4_flex::compress_prepend_size(data);
        if compressed.len() >= data.len() {
            return Err(Lz4CompressError::Unworthy);
        }

        Ok(compressed)
    }
}


impl Lz4Decompress for Vec<u8> {
    #[inline]
    fn lz4_decompress(data: &[u8]) -> Result<Self, Lz4DecompressError> {
        lz4_flex::decompress_size_prepended(data)
    }
}