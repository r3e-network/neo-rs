// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use serde::{Deserialize, Serialize};

use crate::h256::H256;
use neo_base::bytes::Bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum TxAttrType {
    HighPriority = 0x01,
    OracleResponse = 0x11,
    NotValidBefore = 0x20,
    Conflicts = 0x21,
    NotaryAssisted = 0x22,
}

impl TxAttrType {
    pub fn allow_multiple(self) -> bool {
        self == Self::Conflicts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum OracleCode {
    Success = 0x00,
    ProtocolNotSupported = 0x10,
    ConsensusUnreachable = 0x12,
    NotFound = 0x14,
    Timeout = 0x16,
    Forbidden = 0x18,
    ResponseTooLarge = 0x1A,
    InsufficientFunds = 0x1C,
    ContentTypeNotSupported = 0x1F,
    Error = 0xFF,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OracleResponse {
    pub id: u64,
    pub code: OracleCode,
    pub result: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NotValidBefore {
    pub height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Conflicts {
    pub hash: H256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NotaryAssisted {
    pub nkeys: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum TxAttr {
    // #[bin(tag = 0x01)]
    HighPriority,

    // #[bin(tag = 0x11)]
    OracleResponse(OracleResponse),

    // #[bin(tag = 0x20)]
    NotValidBefore(NotValidBefore),

    // #[bin(tag = 0x21)]
    Conflicts(Conflicts),
    // #[bin(tag = 0x22)]
    // NotaryAssisted(NotaryAssisted),
}
