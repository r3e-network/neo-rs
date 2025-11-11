use neo_base::bytes::Bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OracleResponse {
    pub id: u64,
    pub code: OracleCode,
    pub result: Bytes,
}
