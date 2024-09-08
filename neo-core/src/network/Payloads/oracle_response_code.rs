use std::convert::TryFrom;

/// Represents the response code for the oracle request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleResponseCode {
    /// Indicates that the request has been successfully completed.
    Success = 0x00,

    /// Indicates that the protocol of the request is not supported.
    ProtocolNotSupported = 0x10,

    /// Indicates that the oracle nodes cannot reach a consensus on the result of the request.
    ConsensusUnreachable = 0x12,

    /// Indicates that the requested Uri does not exist.
    NotFound = 0x14,

    /// Indicates that the request was not completed within the specified time.
    Timeout = 0x16,

    /// Indicates that there is no permission to request the resource.
    Forbidden = 0x18,

    /// Indicates that the data for the response is too large.
    ResponseTooLarge = 0x1a,

    /// Indicates that the request failed due to insufficient balance.
    InsufficientFunds = 0x1c,

    /// Indicates that the content-type of the request is not supported.
    ContentTypeNotSupported = 0x1f,

    /// Indicates that the request failed due to other errors.
    Error = 0xff,
}

impl TryFrom<u8> for OracleResponseCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(OracleResponseCode::Success),
            0x10 => Ok(OracleResponseCode::ProtocolNotSupported),
            0x12 => Ok(OracleResponseCode::ConsensusUnreachable),
            0x14 => Ok(OracleResponseCode::NotFound),
            0x16 => Ok(OracleResponseCode::Timeout),
            0x18 => Ok(OracleResponseCode::Forbidden),
            0x1a => Ok(OracleResponseCode::ResponseTooLarge),
            0x1c => Ok(OracleResponseCode::InsufficientFunds),
            0x1f => Ok(OracleResponseCode::ContentTypeNotSupported),
            0xff => Ok(OracleResponseCode::Error),
            _ => Err(()),
        }
    }
}

impl From<OracleResponseCode> for u8 {
    fn from(code: OracleResponseCode) -> Self {
        code as u8
    }
}
