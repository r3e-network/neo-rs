use alloc::{boxed::Box, vec::Vec};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoRead},
    hash::Hash160,
};
use neo_crypto::ecc256::PublicKey;

use super::super::WitnessCondition;

impl NeoDecode for WitnessCondition {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0x00 => Ok(WitnessCondition::Boolean {
                expression: reader.read_u8()? != 0,
            }),
            0x01 => Ok(WitnessCondition::Not {
                expression: Box::new(WitnessCondition::neo_decode(reader)?),
            }),
            0x02 => {
                let len = reader.read_varint()? as usize;
                let mut expressions = Vec::with_capacity(len);
                for _ in 0..len {
                    expressions.push(WitnessCondition::neo_decode(reader)?);
                }
                Ok(WitnessCondition::And { expressions })
            }
            0x03 => {
                let len = reader.read_varint()? as usize;
                let mut expressions = Vec::with_capacity(len);
                for _ in 0..len {
                    expressions.push(WitnessCondition::neo_decode(reader)?);
                }
                Ok(WitnessCondition::Or { expressions })
            }
            0x18 => Ok(WitnessCondition::ScriptHash {
                hash: Hash160::neo_decode(reader)?,
            }),
            0x19 => Ok(WitnessCondition::Group {
                group: PublicKey::neo_decode(reader)?,
            }),
            0x20 => Ok(WitnessCondition::CalledByEntry),
            0x28 => Ok(WitnessCondition::CalledByContract {
                hash: Hash160::neo_decode(reader)?,
            }),
            0x29 => Ok(WitnessCondition::CalledByGroup {
                group: PublicKey::neo_decode(reader)?,
            }),
            _ => Err(DecodeError::InvalidValue("WitnessCondition")),
        }
    }
}
