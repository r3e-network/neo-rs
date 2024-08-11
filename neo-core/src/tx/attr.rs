// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use serde::{Deserialize, Serialize};

use neo_base::encoding::bin::*;

use crate::types::{Bytes, H256};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum AttrType {
    HighPriority = 0x01,
    OracleResponse = 0x11,
    NotValidBefore = 0x20,
    Conflicts = 0x21,
    NotaryAssisted = 0x22,
}

impl AttrType {
    pub fn allow_multiple(self) -> bool { self == Self::Conflicts }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
#[bin(repr = u8)]
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

#[derive(Debug, Clone, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct OracleResponse {
    pub id: u64,
    pub code: OracleCode,
    pub result: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct NotValidBefore {
    pub height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct Conflicts {
    pub hash: H256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct NotaryAssisted {
    pub nkeys: u8,
}

// #[derive(Debug, Clone)]
// pub struct Reserved {
//     value: Bytes,
// }

#[derive(Debug, Clone, Deserialize, Serialize, BinEncode, BinDecode)]
#[serde(tag = "type")]
#[bin(repr = u8)]
pub enum TxAttr {
    #[bin(tag = 0x01)]
    HighPriority,

    #[bin(tag = 0x11)]
    OracleResponse(OracleResponse),

    #[bin(tag = 0x20)]
    NotValidBefore(NotValidBefore),

    #[bin(tag = 0x21)]
    Conflicts(Conflicts),

    #[bin(tag = 0x22)]
    NotaryAssisted(NotaryAssisted),
}

impl TxAttr {
    pub fn allow_multiple(&self) -> bool {
        match self {
            TxAttr::Conflicts(_) => true,
            _ => false
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_tx_attr_json() {
        let attr = TxAttr::NotValidBefore(NotValidBefore { height: 123 });

        let attr = serde_json::to_string(&attr)
            .expect("json encode should be ok");

        assert_eq!(&attr, r#"{"type":"NotValidBefore","height":123}"#);
    }
}