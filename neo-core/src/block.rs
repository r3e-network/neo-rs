// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use neo_base::encoding::{bin::*, encode_hex_u64, decode_hex_u64};
use crate::network::Payloads::Header;
use crate::tx::Tx;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

#[derive(Debug, Clone, Serialize, Deserialize, BinEncode, BinDecode)]
pub struct Block {
    header: Header,

    #[serde(rename = "tx")]
    txs: Vec<Tx>,
}

impl Block {
    pub fn new(header: Header, txs: Vec<Tx>) -> Self { Self { header, txs } }

    pub fn hash(&self) -> UInt256 { self.header.hash() }

    pub fn header(&self) -> &Header { &self.header }

    pub fn txs(&self) -> &[Tx] { self.txs.as_slice() }

    pub fn block_index(&self) -> u32 { self.header.index }

    pub fn new_genesis_block(validators: &[PublicKey]) -> Self {
        let next_consensus = validators.to_bft_hash()
            .expect("`to_bft_hash` should be ok");

        // 0x11 is op-code PUSH1
        let witness = Witness::new(Script::default(), Script::from(&b"\x11"[..]));
        let mut header = Header {
            hash: None,
            version: 0,
            prev_hash: Default::default(),
            merkle_root: Default::default(),
            unix_milli: 0,
            nonce: 2083236893, // nonce from the Bitcoin genesis block.
            index: 0,
            primary: 0,
            next_consensus: next_consensus.into(),
            witnesses: witness.into(),
        };
        header.calc_hash();

        Self::new(header, Vec::new())
    }

    pub fn to_trimmed_block(&self) -> TrimmedBlock {
        TrimmedBlock {
            header: self.header.clone(),
            hashes: self.txs.iter().map(|tx| tx.hash()).collect(),
        }
    }
}

impl EncodeHashFields for Block {
    fn encode_hash_fields(&self, w: &mut impl BinWriter) {
        self.header.encode_hash_fields(w);
    }
}


#[derive(Debug, Clone, Serialize, Deserialize, BinEncode, BinDecode)]
pub struct TrimmedBlock {
    pub header: Header,

    /// The hashes of the txs in this block.
    pub hashes: Vec<UInt256>,
}


impl TrimmedBlock {
    pub fn hash(&self) -> UInt256 { self.header.hash() }

    pub fn header(&self) -> &Header { &self.header }

    pub fn block_index(&self) -> u32 { self.header.index }
}


#[derive(Debug, Clone)]
pub struct StatedBlock {
    pub header: Header,

    pub txs: Vec<StatedTx>,
}

impl StatedBlock {
    pub fn hash(&self) -> UInt256 { self.header.hash() }

    pub fn txs(&self) -> &[StatedTx] { &self.txs }

    pub fn block_index(&self) -> u32 { self.header.index }

    pub fn to_trimmed_block(&self) -> TrimmedBlock {
        TrimmedBlock {
            header: self.header.clone(),
            hashes: self.txs.iter().map(|tx| tx.hash()).collect(),
        }
    }
}

/// Block hash and it's height, i.e. `HashIndexState`
#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
pub struct IndexHash {
    pub index: u32,
    pub hash: UInt256,
}

pub trait HashToIndex {
    fn hash_to_index(&self, hash: &UInt256) -> Option<u32>;
}

pub trait IndexToHash {
    fn index_to_hash(&self, height: u32) -> Option<UInt256>;
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::tx::Witness;
    use neo_crypto::{rand::OsRand, secp256r1::GenKeypair};


    #[test]
    fn test_header_encoding() {
        let mut head = Header {
            hash: Default::default(),
            version: 1,
            prev_hash: Default::default(),
            merkle_root: Default::default(),
            unix_milli: 0x99887766,
            nonce: 0x01020304,
            index: 2,
            primary: 1,
            next_consensus: Default::default(),
            witnesses: Witness::new(Default::default(), Default::default()).into(),
        };

        head.calc_hash();
        let encoded = serde_json::to_string(&head)
            .expect("json encode should be ok");

        let got: Header = serde_json::from_str(&encoded)
            .expect("json decode should be ok");

        assert_eq!(head.hash, got.hash);
        assert_eq!(head.version, got.version);
        assert_eq!(head.unix_milli, got.unix_milli);
        assert_eq!(head.nonce, got.nonce);
        assert_eq!(head.index, got.index);
        assert_eq!(head.primary, got.primary);
        // assert!(head.prev_state_root.is_zero());
    }

    #[test]
    fn test_genesis_block() {
        let (_, pk1) = OsRand::gen_keypair(&mut OsRand)
            .expect("gen_keypair should be ok");

        let genesis = Block::new_genesis_block(core::array::from_ref(&pk1));
        assert_eq!(genesis.header.index, 0);
    }
}