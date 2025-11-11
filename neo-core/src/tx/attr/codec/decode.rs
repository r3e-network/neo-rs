use neo_base::{
    bytes::Bytes,
    encoding::{DecodeError, NeoDecode, NeoRead},
};

use crate::h256::H256;

use super::super::{Conflicts, NotValidBefore, NotaryAssisted, OracleCode, OracleResponse, TxAttr};

impl NeoDecode for TxAttr {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0x01 => Ok(TxAttr::HighPriority),
            0x11 => {
                let id = reader.read_u64()?;
                let code = match reader.read_u8()? {
                    0x00 => OracleCode::Success,
                    0x10 => OracleCode::ProtocolNotSupported,
                    0x12 => OracleCode::ConsensusUnreachable,
                    0x14 => OracleCode::NotFound,
                    0x16 => OracleCode::Timeout,
                    0x18 => OracleCode::Forbidden,
                    0x1A => OracleCode::ResponseTooLarge,
                    0x1C => OracleCode::InsufficientFunds,
                    0x1F => OracleCode::ContentTypeNotSupported,
                    0xFF => OracleCode::Error,
                    _ => return Err(DecodeError::InvalidValue("OracleCode")),
                };
                let result = Bytes::from(reader.read_var_bytes(u32::MAX as u64)?);
                Ok(TxAttr::OracleResponse(OracleResponse { id, code, result }))
            }
            0x20 => Ok(TxAttr::NotValidBefore(NotValidBefore {
                height: reader.read_u64()?,
            })),
            0x21 => Ok(TxAttr::Conflicts(Conflicts {
                hash: H256::neo_decode(reader)?,
            })),
            0x22 => Ok(TxAttr::NotaryAssisted(NotaryAssisted {
                nkeys: reader.read_u8()?,
            })),
            _ => Err(DecodeError::InvalidValue("TxAttr")),
        }
    }
}
