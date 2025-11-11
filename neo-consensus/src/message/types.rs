use core::{convert::TryFrom, fmt};
use neo_base::{encoding::DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ViewNumber(pub u32);

impl ViewNumber {
    pub const ZERO: Self = Self(0);
}

impl NeoEncode for ViewNumber {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(self.0);
    }
}

impl NeoDecode for ViewNumber {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let value = reader.read_u32()?;
        Ok(ViewNumber(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageKind {
    PrepareRequest = 0,
    PrepareResponse = 1,
    Commit = 2,
    ChangeView = 3,
}

impl MessageKind {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for MessageKind {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::PrepareRequest),
            1 => Ok(Self::PrepareResponse),
            2 => Ok(Self::Commit),
            3 => Ok(Self::ChangeView),
            _ => Err(DecodeError::InvalidValue("message kind")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChangeViewReason {
    Timeout = 0,
    ChangeAgreement = 1,
    TxNotFound = 2,
    TxRejectedByPolicy = 3,
    TxInvalid = 4,
    BlockRejectedByPolicy = 5,
}

impl ChangeViewReason {
    pub fn from_u8(value: u8) -> Result<Self, DecodeError> {
        match value {
            0 => Ok(Self::Timeout),
            1 => Ok(Self::ChangeAgreement),
            2 => Ok(Self::TxNotFound),
            3 => Ok(Self::TxRejectedByPolicy),
            4 => Ok(Self::TxInvalid),
            5 => Ok(Self::BlockRejectedByPolicy),
            _ => Err(DecodeError::InvalidValue("change view")),
        }
    }
}

impl fmt::Display for ChangeViewReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeViewReason::Timeout => f.write_str("Timeout"),
            ChangeViewReason::ChangeAgreement => f.write_str("ChangeAgreement"),
            ChangeViewReason::TxNotFound => f.write_str("TxNotFound"),
            ChangeViewReason::TxRejectedByPolicy => f.write_str("TxRejectedByPolicy"),
            ChangeViewReason::TxInvalid => f.write_str("TxInvalid"),
            ChangeViewReason::BlockRejectedByPolicy => f.write_str("BlockRejectedByPolicy"),
        }
    }
}
