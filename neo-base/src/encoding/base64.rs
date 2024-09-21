// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};
use core::result::Result;

use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE},
    Engine,
};

use crate::errors;

pub trait ToBase64 {
    fn to_base64_std(&self) -> String;

    fn to_base64_url(&self) -> String;
}

impl<T: AsRef<[u8]>> ToBase64 for T {
    #[inline]
    fn to_base64_std(&self) -> String { STANDARD.encode(self.as_ref()) }

    #[inline]
    fn to_base64_url(&self) -> String { URL_SAFE.encode(self.as_ref()) }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum FromBase64Error {
    #[error("base64: invalid character '{0}'")]
    InvalidChar(char),
    #[error("base64: invalid length")]
    InvalidLength,
    #[error("base64: invalid padding")]
    InvalidPadding,
    #[error("base64: invalid last symbol '{0}'")]
    InvalidLastSymbol(char),
}

impl From<base64::DecodeError> for FromBase64Error {
    fn from(value: base64::DecodeError) -> Self {
        use base64::DecodeError as Error;
        match value {
            Error::InvalidLength => Self::InvalidLength,
            Error::InvalidByte(_, ch) => Self::InvalidChar(ch as char),
            Error::InvalidPadding => Self::InvalidPadding,
            Error::InvalidLastSymbol(_, ch) => Self::InvalidLastSymbol(ch as char),
        }
    }
}

pub trait FromBase64: Sized {
    type Error;

    fn from_base64_std<T: AsRef<[u8]>>(src: T) -> Result<Self, Self::Error>;

    fn from_base64_url<T: AsRef<[u8]>>(src: T) -> Result<Self, Self::Error>;
}

impl FromBase64 for Vec<u8> {
    type Error = FromBase64Error;

    #[inline]
    fn from_base64_std<T: AsRef<[u8]>>(src: T) -> Result<Vec<u8>, Self::Error> {
        STANDARD.decode(src.as_ref()).map_err(FromBase64Error::from)
    }

    #[inline]
    fn from_base64_url<T: AsRef<[u8]>>(src: T) -> Result<Vec<u8>, Self::Error> {
        URL_SAFE.decode(src.as_ref()).map_err(FromBase64Error::from)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_base64() {
        let b = [0xfcu8, 0xfe, 0xfd, 0xfc];
        let r = Vec::from_base64_std(&b).expect_err("decode should be failed");
        assert_eq!(r, FromBase64Error::InvalidChar(0xfcu8 as char));

        let b = [0x1, 0x2, 0x3, 0x4, 0x5, 0x6];
        let d = Vec::from_base64_std(&b.to_base64_std()).expect("decode shou be ok");
        assert_eq!(b.as_slice(), d.as_slice());
    }
}
