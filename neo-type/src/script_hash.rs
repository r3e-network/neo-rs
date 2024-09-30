use neo_base::hash::{Ripemd160, Sha256};
use neo_crypto::secp256r1::PublicKey;

use crate::{CheckSign, H160, SCRIPT_HASH_SIZE};

#[derive(Debug, Default, Hash, Copy, Clone, Eq, PartialEq)]
pub struct ScriptHash(pub [u8; SCRIPT_HASH_SIZE]);

impl AsRef<[u8; SCRIPT_HASH_SIZE]> for ScriptHash {
    #[inline]
    fn as_ref(&self) -> &[u8; SCRIPT_HASH_SIZE] {
        &self.0
    }
}

impl AsRef<[u8]> for ScriptHash {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<H160> for ScriptHash {
    #[inline]
    fn from(value: H160) -> Self {
        Self(value.into())
    }
}

impl Into<H160> for ScriptHash {
    #[inline]
    fn into(self) -> H160 {
        H160::from(self.0)
    }
}

pub trait ToScriptHash {
    fn to_script_hash(&self) -> ScriptHash;
}

impl ToScriptHash for [u8] {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash {
        ScriptHash(self.sha256().ripemd160())
    }
}

impl ToScriptHash for CheckSign {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash {
        ScriptHash(self.sha256().ripemd160())
    }
}

impl ToScriptHash for PublicKey {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash {
        self.to_compressed().as_slice().to_script_hash()
    }
}
