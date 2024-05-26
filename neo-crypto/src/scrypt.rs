// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use zeroize::Zeroizing;

#[derive(Debug, Copy, Clone)]
pub struct ScryptParams {
    pub n: u64,
    pub r: u32,
    pub p: u32,
}

impl core::fmt::Display for ScryptParams {
    #[inline]
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::write!(
            formatter,
            "ScryptParams{{n:{},r:{},p:{}}}",
            self.n,
            self.r,
            self.p
        )
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ScryptDeriveError {
    #[error("scrypt: invalid scrypt params")]
    InvalidParams,

    #[error("scrypt: invalid derived length")]
    InvalidDerivedLength,
}

pub trait DeriveScryptKey {
    fn derive_scrypt_key<const N: usize>(
        &self,
        salt: &[u8],
        scrypt: ScryptParams,
    ) -> Result<Zeroizing<[u8; N]>, ScryptDeriveError>;
}

impl<T: AsRef<[u8]>> DeriveScryptKey for T {
    /// key length must in [10, 64],
    /// n must be power of two,
    /// r must in [1, 4294967295],
    /// p must in [1, 4294967295],
    /// N must be satisfied (N > 0 && N/32 > 0xffff_ffff)
    fn derive_scrypt_key<const N: usize>(
        &self,
        salt: &[u8],
        scrypt: ScryptParams,
    ) -> Result<Zeroizing<[u8; N]>, ScryptDeriveError> {
        if scrypt.n.count_ones() != 1 {
            return Err(ScryptDeriveError::InvalidParams);
        }

        let key = self.as_ref();
        let params = scrypt::Params::new(scrypt.n.ilog2() as u8, scrypt.r, scrypt.p, key.len())
            .map_err(|_err| ScryptDeriveError::InvalidParams)?;

        let mut derived = Zeroizing::new([0u8; N]);
        let _ = scrypt::scrypt(key, salt, &params, derived.as_mut_slice())
            .map_err(|_err| ScryptDeriveError::InvalidDerivedLength)?;

        Ok(derived)
    }
}
