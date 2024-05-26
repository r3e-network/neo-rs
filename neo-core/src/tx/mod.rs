// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

mod attr;
mod signer;
mod verify;
mod witness;

pub use attr::*;
pub use signer::*;
pub use verify::*;
pub use witness::*;

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::h256::H256;
use crate::script::Script;

#[derive(Debug, Clone)]
pub struct Tx {
    /// i.e. tx-id, None means no set. Set it to None if hash-fields changed
    //  #[bin(ignore)]
    // #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    hash: Option<H256>,

    /// None means not-computed. Set it to None if hash-fields changed
    // #[bin(ignore)]
    // #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    size: Option<u32>,

    pub version: u8,
    pub nonce: u32,

    pub sysfee: u64,
    pub netfee: u64,

    //#[serde(rename = "validuntilblock")]
    pub valid_until_block: u32,

    pub signers: Vec<Signer>,

    pub attributes: Vec<TxAttr>,

    pub script: Script,

    /// i.e. scripts
    pub witnesses: Vec<Witness>,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    StateValidator = 4,
    Oracle = 8,
    NeoFSAlphabet = 16,
    P2pNotary = 32,
}
