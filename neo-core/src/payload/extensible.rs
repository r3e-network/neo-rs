// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::String;

use serde::{Deserialize, Serialize};

use neo_base::encoding::bin::*;
use neo_base::hash::Sha256;
use neo_crypto::ecdsa::{Sign as EcdsaSign, SignError};

use crate::PrivateKey;
use crate::tx::Witnesses;
use crate::types::{Bytes, H160, Sign, ToSignData};

pub const CONSENSUS_CATEGORY: &'static str = "dBFT";
pub const MAX_CATEGORY_SIZE: usize = 32;
pub const MAX_DATA_SIZE: usize = 0x02000000;

#[derive(Debug, Clone, Serialize, Deserialize, BinEncode, InnerBinDecode)]
pub struct Extensible {
    pub category: String,
    pub valid_block_start: u32,
    pub valid_block_end: u32,
    pub sender: H160,
    pub data: Bytes,
    pub witnesses: Witnesses,
}

impl Extensible {
    pub fn sign(&mut self, network: u32, key: &PrivateKey) -> Result<Sign, SignError> {
        let hash = self.to_sign_data(network).sha256();
        key.sign(hash.as_slice()).map(|sign| Sign::from(sign))
    }
}

impl EncodeHashFields for Extensible {
    fn encode_hash_fields(&self, w: &mut impl BinWriter) {
        self.category.encode_bin(w);
        self.valid_block_start.encode_bin(w);
        self.valid_block_end.encode_bin(w);
        self.sender.encode_bin(w);
        self.data.encode_bin(w);
    }
}

impl BinDecoder for Extensible {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let ext = Self::decode_bin_inner(r)?;
        let len = ext.category.len();
        if len > MAX_CATEGORY_SIZE {
            return Err(BinDecodeError::InvalidLength("Extensible", "category", len));
        }

        let len = ext.data.len();
        if len > MAX_DATA_SIZE {
            return Err(BinDecodeError::InvalidLength("Extensible", "data", len));
        }

        Ok(ext)
    }
}

#[cfg(test)]
mod test {
    use bytes::BytesMut;

    use neo_base::hash::{Ripemd160, Sha256};

    use crate::tx::Witness;

    use super::*;

    #[test]
    fn test_extensible() {
        let ext = Extensible {
            category: "Mock".into(),
            valid_block_start: 1,
            valid_block_end: 10,
            sender: "Hello".sha256().ripemd160().into(),
            data: b"Hello".to_vec().into(),
            witnesses: Witness::new(b"invocation".as_ref().into(), b"verification".as_ref().into())
                .into(),
        };

        let mut w = BytesMut::with_capacity(128);
        ext.encode_bin(&mut w);

        let mut r = RefBuffer::from(w.as_ref());
        let got: Extensible = BinDecoder::decode_bin(&mut r).expect("decode_bin should be ok");

        assert_eq!(ext.category, got.category);
        assert_eq!(ext.valid_block_end, got.valid_block_end);
        assert_eq!(ext.valid_block_start, got.valid_block_start);
        assert_eq!(ext.sender, got.sender);
        assert_eq!(ext.data, got.data);

        let wit = ext.witnesses.witness();
        let got = got.witnesses.witness();
        assert_eq!(wit.invocation_script, got.invocation_script);
        assert_eq!(wit.verification_script, got.verification_script);
    }
}
