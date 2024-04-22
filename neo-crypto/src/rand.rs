// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::ToString;
use neo_base::errors;

pub trait CryptoRand {
    type Error: ToString;

    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), Self::Error>;

    #[inline]
    fn read_u64(&mut self) -> Result<u64, Self::Error> {
        let mut buf = [0u8; 8];
        let _read = self.read_full(&mut buf)?;

        Ok(u64::from_be_bytes(buf))
    }
}


#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum RandError {
    #[error("crypto-rand: gen random error {0}")]
    GenRandomError(i32),
}

pub struct OsRand;

impl CryptoRand for OsRand {
    type Error = RandError;

    #[inline]
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        use rand_core::{RngCore, OsRng};

        OsRng.try_fill_bytes(buf)
            .map_err(|e| RandError::GenRandomError(e.raw_os_error().unwrap_or(0)))
    }
}

/// NOTE: buf length cannot too long.
#[inline]
pub fn rand_bytes(buf: &mut [u8]) -> Result<(), RandError> {
    OsRand.read_full(buf)
}

#[inline]
pub fn read_u64() -> Result<u64, RandError> {
    OsRand.read_u64()
}