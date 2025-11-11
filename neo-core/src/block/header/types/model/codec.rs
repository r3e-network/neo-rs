use alloc::vec::Vec;

use neo_base::encoding::{DecodeError, NeoDecode, NeoRead};

use crate::{
    h160::H160,
    h256::H256,
    io::{self},
    tx::Witness,
};

use super::Header;

impl NeoDecode for Header {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let raw_version = reader.read_u32()?;
        let state_root_enabled = (raw_version & super::STATE_ROOT_FLAG) != 0;
        let version = raw_version & !super::STATE_ROOT_FLAG;
        let prev_hash = H256::neo_decode(reader)?;
        let merkle_root = H256::neo_decode(reader)?;
        let unix_milli = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary = reader.read_u8()?;
        let next_consensus = H160::neo_decode(reader)?;
        let prev_state_root = if state_root_enabled {
            Some(H256::neo_decode(reader)?)
        } else {
            None
        };
        let witnesses: Vec<Witness> = io::read_array(reader)?;
        if witnesses.is_empty() {
            return Err(DecodeError::InvalidValue("HeaderWitness"));
        }
        Ok(Self {
            hash: None,
            version,
            prev_hash,
            merkle_root,
            unix_milli,
            nonce,
            index,
            primary,
            next_consensus,
            witnesses,
            state_root_enabled,
            prev_state_root,
        })
    }
}
