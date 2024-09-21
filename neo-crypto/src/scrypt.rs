// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::errors;

use crate::key::SecretKey;

#[derive(Debug, Clone, errors::Error)]
pub enum Error {
    #[error("scrypt: invalid params")]
    InvalidParams,

    #[error("scrypt: invalid derived length")]
    InvalidDerivedLength,
}

#[derive(Debug, Copy, Clone)]
pub struct Params {
    pub n: u64,
    pub r: u32,
    pub p: u32,
    pub len: u32,
}

/// impl Display for Params
impl core::fmt::Display for Params {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::write!(f, "Params{{n:{},r:{},p:{},len:{}}}", self.n, self.r, self.p, self.len)
    }
}

pub trait DeriveKey {
    fn derive_key<const N: usize>(
        &self,
        salt: &[u8],
        params: Params,
    ) -> Result<SecretKey<N>, Error>;
}

impl<T: AsRef<[u8]>> DeriveKey for T {
    /// key length must in [10, 64],
    /// n must be power of two,
    /// r must in [1, 4294967295],
    /// p must in [1, 4294967295],
    /// N must be satisfied (N > 0 && N/32 > 0xffff_ffff)
    fn derive_key<const N: usize>(
        &self,
        salt: &[u8],
        params: Params,
    ) -> Result<SecretKey<N>, Error> {
        if params.n.count_ones() != 1 {
            return Err(Error::InvalidParams);
        }

        let key = self.as_ref();
        let params =
            scrypt::Params::new(params.n.ilog2() as u8, params.r, params.p, params.len as usize)
                .map_err(|_| Error::InvalidParams)?;

        let mut derived = [0u8; N];
        let _ = scrypt::scrypt(key, salt, &params, derived.as_mut_slice())
            .map_err(|_| Error::InvalidDerivedLength)?;

        Ok(derived.into())
    }
}

#[cfg(test)]
mod test {
    use neo_base::encoding::hex::{DecodeHex, ToHex};

    use super::*;

    #[test]
    fn test_derive_key() {
        let password = b"1234567890";
        let salt = "fd4acb81182a2c8fa959d180967b374277f2ccf2f7f401cb08d042cc785464b4"
            .decode_hex()
            .expect("decode hex should be ok");

        let params = Params { n: 2, r: 8, p: 1, len: 10 };
        let key: SecretKey<64> = password
            .derive_key(salt.as_slice(), params)
            .expect("A 64-bytes derived-key should be ok");

        let expected = "52a5dacfcf80e5111d2c7fbed177113a1b48a882b066a017f2c856086680fac7\
            43ae0dd1ba325be061003ec144f1cad75ddbadd7bb01d22970b9904720b6ba27";

        assert_eq!(&key.to_hex(), expected);
    }
}
