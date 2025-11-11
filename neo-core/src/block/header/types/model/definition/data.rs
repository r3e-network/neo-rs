use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{block::header::types::ser, h160::H160, h256::H256, tx::Witness};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<H256>,
    pub version: u32,
    #[serde(rename = "previousblockhash")]
    pub prev_hash: H256,
    #[serde(rename = "merkleroot")]
    pub merkle_root: H256,
    #[serde(rename = "time")]
    pub unix_milli: u64,
    #[serde(
        serialize_with = "ser::encode_hex_u64",
        deserialize_with = "ser::decode_hex_u64"
    )]
    pub nonce: u64,
    pub index: u32,
    #[serde(rename = "primary")]
    pub primary: u8,
    #[serde(rename = "nextconsensus")]
    pub next_consensus: H160,
    pub witnesses: Vec<Witness>,
    #[serde(skip)]
    pub state_root_enabled: bool,
    #[serde(
        default,
        rename = "previousstateroot",
        skip_serializing_if = "Option::is_none"
    )]
    pub prev_state_root: Option<H256>,
}
