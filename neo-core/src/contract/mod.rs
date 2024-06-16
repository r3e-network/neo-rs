// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{vec, vec::Vec};
use ::bytes::{BufMut, BytesMut};

use neo_base::{errors, encoding::bin::*};
use crate::{PublicKey, PUBLIC_COMPRESSED_SIZE, types::*};

pub mod context;

pub mod event;
pub mod manifest;

pub mod nef;
pub mod nep17;
pub mod nep11;

pub mod natives;

pub mod param;

pub use {context::*, event::*, manifest::*, nef::*, nep17::*, nep11::*, natives::*, param::*};


pub const MIN_MULTI_CONTRACT_SIZE: usize = 42;


#[derive(Debug)]
pub struct Contract {
    pub script: Script,
    pub params: Vec<ParamType>,
    pub script_hash: H160,
}


impl Contract {
    #[inline]
    pub fn as_bytes(&self) -> &[u8] { self.script.as_bytes() }
}

pub trait ToSignContract {
    fn to_sign_contract(&self) -> Contract;
}

impl<T: ToCheckSign> ToSignContract for T {
    fn to_sign_contract(&self) -> Contract {
        let script = self.to_check_sign();
        let hash = script.to_script_hash();
        Contract {
            script: script.into(),
            params: vec![ParamType::Signature],
            script_hash: hash.into(),
        }
    }
}

#[derive(Debug, Copy, Clone, errors::Error)]
pub enum ToMultiSignError {
    #[error("to-multi-sign: invalid members '{0}'")]
    InvalidMembers(usize),

    #[error("to-multi-sign: invalid signers '{0}'")]
    InvalidSigners(usize),
}

pub trait ToMultiSignContract {
    /// NOTE: must check self(i.e. public-keys) is/are not empty and signers must in [1, keys.len]
    fn to_multi_sign_contract(&self, signers: u32) -> Result<Contract, ToMultiSignError>;
}

impl<T: AsRef<[PublicKey]>> ToMultiSignContract for T {
    fn to_multi_sign_contract(&self, signers: u32) -> Result<Contract, ToMultiSignError> {
        let keys = self.as_ref();
        if keys.is_empty() || keys.len() > MAX_SIGNERS {
            return Err(ToMultiSignError::InvalidMembers(keys.len()));
        }

        let signers = signers as usize;
        if signers <= 0 || signers > keys.len() {
            return Err(ToMultiSignError::InvalidSigners(signers));
        }

        let script = keys.to_check_multi_sign(signers as u16);
        let hash = script.to_script_hash();
        Ok(Contract {
            script: script.into(),
            params: vec![ParamType::Signature; signers],
            script_hash: hash.into(),
        })
    }
}


pub fn contract_hash(sender: &H160, name: &str, nef_checksum: u32) -> H160 {
    const OPCODE_ABORT: u8 = 0x38;

    // 1(Abort) + (2+20) + (2+name.len()) + 4
    let mut buf = BytesMut::with_capacity(1 + (2 + 20) + (2 + name.len()) + 4);

    buf.put_u8(OPCODE_ABORT);
    buf.put_varbytes(sender.as_le_bytes());
    buf.put_varint(nef_checksum as u64);
    buf.put_varbytes(name);

    buf.to_script_hash().into()
}


pub trait IsSignContract {
    fn is_sign_contract(&self) -> bool;
}

impl<T: AsRef<[u8]>> IsSignContract for T {
    fn is_sign_contract(&self) -> bool {
        let bytes = self.as_ref();
        bytes.len() == CHECK_SIG_SIZE &&
            bytes[0] == PUSH_DATA1 &&
            bytes[1] == PUBLIC_COMPRESSED_SIZE as u8 &&
            bytes[35] == CHECK_SIG_OP_CODE &&
            bytes.ends_with(&CHECK_SIG_HASH_SUFFIX)
    }
}

impl IsSignContract for Contract {
    #[inline]
    fn is_sign_contract(&self) -> bool {
        self.as_bytes().is_sign_contract()
    }
}


pub trait IsMultiSignContract {
    /// It determines a contract is multi-sign contract or not,
    /// and returns (public-keys-number, signers-number) when it is.
    fn is_multi_sign_contract(&self) -> Option<(u16, u16)>;
}

impl<T: AsRef<[u8]>> IsMultiSignContract for T {
    fn is_multi_sign_contract(&self) -> Option<(u16, u16)> {
        let bytes = self.as_ref();
        if bytes.len() < MIN_MULTI_CONTRACT_SIZE {
            return None;
        }

        let mut r = RefBuffer::from(bytes);
        let signers = match u8::decode_bin(&mut r).ok()? {
            0x00 => u8::decode_bin(&mut r).ok()? as u16, // PUSH INT8
            0x01 => u16::decode_bin(&mut r).ok()?, // PUSH INT16
            n if n >= 0x11 && n <= 0x20 => (n - 0x11) as u16, // 0x11 -> PUSH0
            _ => return None,
        };

        if signers < 1 || signers > MAX_SIGNERS as u16 {
            return None;
        }

        let mut keys = 0usize;
        while u8::decode_bin(&mut r).ok()? == PUSH_DATA1 {
            let size = u8::decode_bin(&mut r).ok()?;
            if size != PUBLIC_COMPRESSED_SIZE as u8 {
                return None;
            }

            if r.discard(PUBLIC_COMPRESSED_SIZE) < PUBLIC_COMPRESSED_SIZE { // just discard
                return None;
            }
            keys += 1;
        }

        if keys < signers as usize || keys > MAX_SIGNERS {
            return None;
        }

        let n = match u8::decode_bin(&mut r).ok()? {
            0x00 => u8::decode_bin(&mut r).ok()? as u16, // PUSH INT8
            0x01 => u16::decode_bin(&mut r).ok()?, // PUSH INT16
            n if n >= 0x11 && n <= 0x20 => (n - 0x11) as u16, // 0x11 -> PUSH0
            _ => return None,
        };

        if n != keys as u16 {
            return None;
        }

        if u8::decode_bin(&mut r).ok()? != CHECK_SIG_OP_CODE {
            return None;
        }

        r.as_bytes().eq(&CHECK_MULTI_SIG_HASH_SUFFIX).then_some((keys as u16, signers))
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use neo_base::encoding::hex::DecodeHex;
    use neo_crypto::{rand::OsRand, secp256r1::GenKeypair};
    use crate::PublicKey;

    #[test]
    fn test_sig_contract() {
        let x = "035a928f201639204e06b4368b1a93365462a8ebbff0b8818151b74faab3a2b61a"
            .decode_hex()
            .expect("hex decode should be ok");

        let key = PublicKey::from_compressed(&x)
            .expect("from compressed public should be ok");

        let contract = key.to_sign_contract();
        assert_eq!(contract.script.as_bytes(), key.to_check_sign().as_bytes());
        assert_eq!(contract.params.len(), 1);
        assert_eq!(contract.params[0], ParamType::Signature);
        assert_eq!(contract.script_hash, key.to_check_sign().to_script_hash().into());
    }

    #[test]
    fn test_multi_sig_contract() {
        let (_, pk1) = OsRand::gen_keypair(&mut OsRand)
            .expect("gen_keypair should be ok");

        let (_, pk2) = OsRand::gen_keypair(&mut OsRand)
            .expect("gen_keypair should be ok");

        let keys = [pk1, pk2];
        let _ = keys.to_multi_sign_contract(0)
            .expect_err("cannot be zero signer");

        let _ = keys.to_multi_sign_contract(3)
            .expect_err("cannot be zero signer");

        let _ = keys.to_multi_sign_contract(1)
            .expect("to_multi_sign_contract should be ok");
    }
}