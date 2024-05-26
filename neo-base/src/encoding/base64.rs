// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;
use alloc::vec::Vec;

use base64::{Engine, engine::general_purpose::STANDARD};

pub trait ToBase64 {
    fn to_base64_std(&self) -> String;
}

impl<T: AsRef<[u8]>> ToBase64 for T {
    #[inline]
    fn to_base64_std(&self) -> String {
        STANDARD.encode(self.as_ref())
    }
}

#[derive(Debug, Copy, Clone, thiserror::Error)]
pub enum FromBase64Error {
    #[error("base64: invalid character '{0}'")]
    InvalidChar(char),

    #[error("base64: invalid length({0})")]
    InvalidLength(usize),

    #[error("base64: invalid padding")]
    InvalidPadding,
}

pub trait FromBase64: Sized {
    type Error;

    fn from_base64_std<T: AsRef<[u8]>>(src: &T) -> Result<Self, Self::Error>;
}

impl From<base64::DecodeError> for FromBase64Error {
    fn from(value: base64::DecodeError) -> Self {
        use base64::DecodeError as Error;
        match value {
            Error::InvalidLength(len) => Self::InvalidLength(len),
            Error::InvalidByte(_, ch) => Self::InvalidChar(ch as char),
            Error::InvalidPadding => Self::InvalidPadding,
            Error::InvalidLastSymbol(_, ch) => Self::InvalidChar(ch as char),
        }
    }
}

impl FromBase64 for Vec<u8> {
    type Error = FromBase64Error;

    #[inline]
    fn from_base64_std<T: AsRef<[u8]>>(src: &T) -> Result<Vec<u8>, Self::Error> {
        STANDARD.decode(src.as_ref()).map_err(FromBase64Error::from)
    }
}
