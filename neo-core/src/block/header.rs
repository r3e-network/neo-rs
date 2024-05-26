// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{h160::H160, tx::Tx};
use crate::h256::H256;
use crate::tx::Witnesses;
use neo_base::encoding::{decode_hex_u64, encode_hex_u64};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    /// the hash of this block header
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


#[derive(Debug, Clone)]
pub struct Block {
    header: Header,

    // #[serde(rename = "tx")]
    txs: Vec<Tx>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrimmedBlock {
    pub header: Header,

    /// The hashes of the txs in this block.
    pub hashes: Vec<H256>,
}