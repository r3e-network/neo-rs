// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use bytes::{BytesMut, BufMut};
use neo_base::{byzantine_honest_quorum, errors};

use neo_base::hash::{Ripemd160, Sha256};
use crate::{
    PublicKey, PUBLIC_COMPRESSED_SIZE,
    types::{Bytes, MAX_SIGNERS, Script, ScriptHash, ToScriptHash, Varint},
};


// 40 bytes = 1-byte CHECK_SIG_PUSH_DATA1 + 1-byte length + 33-bytes key + 1-byte OpCode + 4-bytes suffix
pub const CHECK_SIG_SIZE: usize = 1 + 1 + 33 + 1 + 4;

pub const PUSH_DATA1: u8 = 0x0c;
pub const CHECK_SIG_OP_CODE: u8 = 0x41; // i.e syscall opcode

pub const CHECK_SIG_HASH_SUFFIX: [u8; 4] = [0x56, 0xe7, 0xb3, 0x27];
pub const CHECK_MULTI_SIG_HASH_SUFFIX: [u8; 4] = [0x9e, 0xd0, 0xdc, 0x3a];


pub struct CheckSign(pub(crate) [u8; CHECK_SIG_SIZE]);


impl CheckSign {
    #[inline]
    pub fn as_bytes(&self) -> &[u8] { self.0.as_slice() }
}

impl AsRef<[u8; CHECK_SIG_SIZE]> for CheckSign {
    #[inline]
    fn as_ref(&self) -> &[u8; CHECK_SIG_SIZE] { &self.0 }
}

impl AsRef<[u8]> for CheckSign {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl Into<Bytes> for CheckSign {
    #[inline]
    fn into(self) -> Bytes { Bytes::from(self.0.to_vec()) }
}

impl Into<Script> for CheckSign {
    #[inline]
    fn into(self) -> Script { Script::from(self.as_bytes()) }
}


pub struct MultiCheckSign {
    /// the number of public keys
    keys: usize,

    /// the number of signers
    signers: usize,

    sign: Vec<u8>,
}

impl MultiCheckSign {
    #[inline]
    pub(crate) fn new(keys: usize, signers: usize, sign: Vec<u8>) -> Self {
        Self { keys, signers, sign }
    }

    #[inline]
    pub fn keys(&self) -> usize { self.keys }

    #[inline]
    pub fn signers(&self) -> usize { self.signers }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] { self.sign.as_slice() }
}

impl AsRef<[u8]> for MultiCheckSign {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.sign.as_ref() }
}

impl ToScriptHash for MultiCheckSign {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash {
        ScriptHash(self.sign.sha256().ripemd160())
    }
}

impl Into<Bytes> for MultiCheckSign {
    #[inline]
    fn into(self) -> Bytes { Bytes::from(self.sign) }
}

impl Into<Script> for MultiCheckSign {
    #[inline]
    fn into(self) -> Script { Script::from(self.sign) }
}

pub trait ToCheckSign {
    fn to_check_sign(&self) -> CheckSign;
}

impl ToCheckSign for PublicKey {
    fn to_check_sign(&self) -> CheckSign {
        let mut buf = [0u8; CHECK_SIG_SIZE];

        const SIZE: usize = PUBLIC_COMPRESSED_SIZE;
        buf[0] = PUSH_DATA1;
        buf[1] = SIZE as u8;

        buf[2..2 + SIZE].copy_from_slice(self.to_compressed().as_slice());

        buf[2 + SIZE] = CHECK_SIG_OP_CODE;
        buf[3 + SIZE..].copy_from_slice(&CHECK_SIG_HASH_SUFFIX);

        CheckSign(buf)
    }
}

pub trait ToCheckMultiSign {
    /// NOTE: caller must checking signers in (0, keys.len()] otherwise it will panic
    fn to_check_multi_sign(&self, signers: u16) -> MultiCheckSign;
}

impl<T: AsRef<[PublicKey]>> ToCheckMultiSign for T {
    fn to_check_multi_sign(&self, signers: u16) -> MultiCheckSign {
        let keys = self.as_ref();
        let n = keys.len();
        let signers = signers as usize;
        if signers <= 0 || signers > n {
            core::panic!("signers {} is invalid of the number of public-keys {}", signers, n);
        }

        const SIZE: usize = PUBLIC_COMPRESSED_SIZE;
        let mut buf = BytesMut::with_capacity((2 + SIZE) * signers + 64);
        let mut keys = keys.iter().collect::<Vec<_>>();

        keys.sort_by(|lhs, rhs| lhs.cmp(rhs));

        buf.put_varint(signers as u64);
        keys.into_iter()
            .for_each(|k| {
                buf.put_u8(PUSH_DATA1);
                buf.put_u8(SIZE as u8);
                buf.put_slice(k.to_compressed().as_slice());
            });

        buf.put_varint(n as u64);
        buf.put_u8(CHECK_SIG_OP_CODE);
        buf.put_slice(&CHECK_MULTI_SIG_HASH_SUFFIX);

        MultiCheckSign::new(n, signers, Vec::from(buf))
    }
}


#[derive(Debug, Copy, Clone, errors::Error)]
pub enum ToBftHashError {
    #[error("to-bft-address: invalid members '{0}'")]
    InvalidMembers(usize)
}

pub trait ToBftHash {
    fn to_bft_hash(&self) -> Result<ScriptHash, ToBftHashError>;
}

impl<T: AsRef<[PublicKey]>> ToBftHash for T {
    /// NOTE: the number of public-keys must greater than 0 and less than MAX_SIGNERS
    fn to_bft_hash(&self) -> Result<ScriptHash, ToBftHashError> {
        let keys = self.as_ref();
        if keys.is_empty() || keys.len() > MAX_SIGNERS {
            return Err(ToBftHashError::InvalidMembers(keys.len()));
        }

        let honest = byzantine_honest_quorum(keys.len() as u32);
        Ok(keys.to_check_multi_sign(honest as u16).to_script_hash())
    }
}


#[cfg(test)]
mod test {
    use alloc::vec::Vec;
    use neo_base::encoding::hex::DecodeHex;
    use neo_crypto::secp256r1::PublicKey;
    use crate::types::{ToNeo3Address, ToCheckMultiSign, ToCheckSign, ToScriptHash};


    #[test]
    fn test_one_key_address() {
        let pk = "031d8e1630ce640966967bc6d95223d21f44304133003140c3b52004dc981349c9"
            .decode_hex()
            .expect("decode hex should be ok");

        let pk = PublicKey::from_compressed(pk.as_slice())
            .expect("decode key should be ok");

        let addr = pk.to_check_sign()
            .to_script_hash()
            .to_neo3_address();
        assert_eq!(addr.as_str(), "NMBfzaEq2c5zodiNbLPoohVENARMbJim1r");

        let addr = pk.to_check_sign().to_neo3_address();
        assert_eq!(addr.as_str(), "NMBfzaEq2c5zodiNbLPoohVENARMbJim1r");
    }

    #[test]
    fn test_multi_keys_address() {
        let keys = [
            "03cdb067d930fd5adaa6c68545016044aaddec64ba39e548250eaea551172e535c",
            "036c8431cc78b33177a60b4bcc02baf60d05fee5038e7339d3a688e394c2cbd843",
        ].into_iter()
            .map(|x| x.decode_hex().expect("decode hex should be ok"))
            .map(|x| PublicKey::from_compressed(&x).expect("decode key should be ok"))
            .collect::<Vec<_>>();

        let addr = keys.to_check_multi_sign(1).to_neo3_address();
        assert_eq!(addr.as_str(), "NVz3NkQQGGhjM1HxHp6ZpXL3EKCHeKvarv");
    }

    #[test]
    fn test_check_sig() {
        let pk = "03cdb067d930fd5adaa6c68545016044aaddec64ba39e548250eaea551172e535c"
            .decode_hex()
            .expect("decode hex should be ok");

        let pk = PublicKey::from_compressed(pk.as_slice())
            .expect("decode key should be ok");

        let addr = pk.to_neo3_address();
        assert_eq!(addr.as_str(), "NNLi44dJNXtDNSBkofB48aTVYtb1zZrNEs");
    }
}