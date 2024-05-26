// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;
use alloc::vec::Vec;

use crate::hash::Sha256;

pub trait ToBase58Check {
    fn to_base58_check(&self) -> String;
}

impl<T: AsRef<[u8]>> ToBase58Check for T {
    fn to_base58_check(&self) -> String {
        let mut buf = Vec::with_capacity(1 + self.as_ref().len() + 1 + 4);
        buf.extend(self.as_ref());

        let check = buf.sha256().sha256();
        buf.extend(&check[..4]);
        bs58::encode(buf).into_string()
    }
}

pub trait FromBase58Check: Sized {
    type Error;

    fn from_base58_check<T: AsRef<str>>(src: T) -> Result<Self, Self::Error>;
}

#[derive(Debug, Copy, Clone, thiserror::Error)]
pub enum FromBase58CheckError {
    #[error("base58check: invalid base58 encoding")]
    InvalidEncoding,

    #[error("base58check: invalid length")]
    InvalidLength,

    #[error("base58check: invalid checksum")]
    InvalidChecksum,
}

impl FromBase58Check for Vec<u8> {
    type Error = FromBase58CheckError;

    fn from_base58_check<T: AsRef<str>>(src: T) -> Result<Vec<u8>, Self::Error> {
        const MIN_SIZE: usize = 5;
        const START_AT: usize = 0;

        let decoded = bs58::decode(src.as_ref())
            .into_vec()
            .map_err(|err| match err {
                bs58::decode::Error::BufferTooSmall => Self::Error::InvalidLength,
                _ => Self::Error::InvalidEncoding,
            })?;

        if decoded.len() < MIN_SIZE {
            return Err(Self::Error::InvalidLength);
        }

        let sha = (&decoded[..decoded.len() - 4]).sha256().sha256();
        if sha[..4] != decoded[decoded.len() - 4..] {
            return Err(Self::Error::InvalidChecksum);
        }

        Ok(decoded[START_AT..decoded.len() - 4].into())
    }
}
