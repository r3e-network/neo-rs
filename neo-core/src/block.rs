// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use neo_base::encoding::{bin::*, encode_hex_u64, decode_hex_u64};
use crate::{PublicKey, types::{H160, H256, Script, ToBftHash}, tx::{Witnesses, Witness, Tx}};
use crate::tx::StatedTx;


#[derive(Debug, Clone, Serialize, Deserialize, BinEncode, InnerBinDecode)]
pub struct Header {
    /// the hash of this block header
    #[bin(ignore)]
    pub hash: Option<H256>,

    /// the version of this block header
    pub version: u32,

    /// the hash of the previous block.
    #[serde(rename = "previousblockhash")]
    pub prev_hash: H256,

    /// the root hash of a transaction list.
    #[serde(rename = "merkleroot")]
    pub merkle_root: H256,

    /// unix timestamp in milliseconds, i.e. timestamp
    #[serde(rename = "time")]
    pub unix_milli: u64,

    /// a random number
    #[serde(serialize_with = "encode_hex_u64", deserialize_with = "decode_hex_u64")]
    pub nonce: u64,

    /// index/height of the block
    pub index: u32,

    /// the index of the primary consensus node for this block.
    pub primary: u8,

    /// contract address of the next miner
    #[serde(rename = "nextconsensus")]
    pub next_consensus: H160,

    /// Script used to validate the block. Only one is supported at now.
    pub witnesses: Witnesses,

    // #[serde(skip)]
    // pub state_root_enabled: bool,

    // /// the state root of the previous block.
    // #[serde(default, rename = "previousstateroot", skip_serializing_if = "H256::is_zero")]
    // pub prev_state_root: H256,
}

impl Header {
    #[inline]
    pub fn hash(&self) -> H256 {
        self.hash.unwrap_or_else(|| self.hash_fields_sha256().into())
    }

    pub fn calc_hash(&mut self) {
        self.hash = Some(self.hash_fields_sha256().into());
    }
}

impl EncodeHashFields for Header {
    fn encode_hash_fields(&self, w: &mut impl BinWriter) {
        self.version.encode_bin(w); // 4
        self.prev_hash.encode_bin(w); // 32
        self.merkle_root.encode_bin(w); // 32
        self.unix_milli.encode_bin(w); // 8
        self.nonce.encode_bin(w); // 8
        self.index.encode_bin(w);  // 4
        self.primary.encode_bin(w); // 1
        self.next_consensus.encode_bin(w); // 20
        // if self.state_root_enabled {
        //     self.prev_state_root.encode_bin(w);
        // }
    }
}

impl BinDecoder for Header {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut head = Self::decode_bin_inner(r)?;
        head.calc_hash();
        Ok(head)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, BinEncode)]
pub struct Block {
    header: Header,

    #[serde(rename = "tx")]
    txs: Vec<Tx>,
}

impl Block {
    pub fn new(header: Header, txs: Vec<Tx>) -> Self { Self { header, txs } }

    pub fn hash(&self) -> H256 { self.header.hash() }

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
    pub hashes: Vec<H256>,
}


impl TrimmedBlock {
    pub fn hash(&self) -> H256 { self.header.hash() }

    pub fn header(&self) -> &Header { &self.header }

    pub fn block_index(&self) -> u32 { self.header.index }
}


#[derive(Debug, Clone)]
pub struct StatedBlock {
    pub header: Header,

    pub txs: Vec<StatedTx>,
}

impl StatedBlock {
    pub fn hash(&self) -> H256 { self.header.hash() }

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
    pub hash: H256,
}

pub trait HashToIndex {
    fn hash_to_index(&self, hash: &H256) -> Option<u32>;
}

pub trait IndexToHash {
    fn index_to_hash(&self, height: u32) -> Option<H256>;
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