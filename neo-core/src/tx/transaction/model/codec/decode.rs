use alloc::vec::Vec;

use neo_base::encoding::{DecodeError, NeoDecode, NeoRead};

use crate::{
    io,
    script::Script,
    tx::{Signer, TxAttr, Witness},
};

use super::super::Tx;

impl Tx {
    pub(crate) fn decode_unsigned<R: NeoRead>(
        reader: &mut R,
    ) -> Result<(u8, u32, u32, u64, u64, Vec<Signer>, Vec<TxAttr>, Script), DecodeError> {
        let version = reader.read_u8()?;
        let nonce = reader.read_u32()?;
        let valid_until_block = reader.read_u32()?;
        let sysfee = reader.read_u64()?;
        let netfee = reader.read_u64()?;
        let signers = io::read_array(reader)?;
        let attrs = io::read_array(reader)?;
        let script = Script::neo_decode(reader)?;
        Ok((
            version,
            nonce,
            valid_until_block,
            sysfee,
            netfee,
            signers,
            attrs,
            script,
        ))
    }
}

impl NeoDecode for Tx {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let (version, nonce, valid_until_block, sysfee, netfee, signers, attributes, script) =
            Self::decode_unsigned(reader)?;
        let witnesses: Vec<Witness> = io::read_array(reader)?;
        Ok(Self {
            version,
            nonce,
            valid_until_block,
            sysfee,
            netfee,
            signers,
            attributes,
            script,
            witnesses,
        })
    }
}
