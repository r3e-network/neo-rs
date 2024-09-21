// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};

pub use base58::{FromBase58, FromBase58Error, ToBase58};

use crate::{errors, hash::Sha256Twice};

pub trait ToBase58Check {
    fn to_base58_check(&self, prefix: Option<u8>, suffix: Option<u8>) -> String;
}

impl<T: AsRef<[u8]>> ToBase58Check for T {
    fn to_base58_check(&self, prefix: Option<u8>, suffix: Option<u8>) -> String {
        let src = self.as_ref();
        let mut buf = Vec::with_capacity(1 + src.len() + 1 + 4);

        if let Some(prefix) = prefix {
            buf.push(prefix);
        }

        buf.extend(src);

        if let Some(suffix) = suffix {
            buf.push(suffix)
        }

        let check = buf.sha256_twice();
        buf.extend(&check[..4]);

        buf.to_base58()
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum FromBase58CheckError {
    #[error("base58check: invalid character '{0}'")]
    InvalidChar(char),

    #[error("base58check: invalid length")]
    InvalidLength,

    #[error("base58check: invalid checksum")]
    InvalidChecksum,

    #[error("base58check: expected prefix {0} but got {1}")]
    InvalidPrefix(u8, u8),

    #[error("base58check: expected suffix {0} but got {1}")]
    InvalidSuffix(u8, u8),
}

pub trait FromBase58Check: Sized {
    type Error;

    fn from_base58_check<T: AsRef<str>>(
        src: T,
        prefix: Option<u8>,
        suffix: Option<u8>,
    ) -> Result<Self, Self::Error>;
}

impl FromBase58Check for Vec<u8> {
    type Error = FromBase58CheckError;

    fn from_base58_check<T: AsRef<str>>(
        src: T,
        prefix: Option<u8>,
        suffix: Option<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        use base58::FromBase58Error as Error;

        let src = src.as_ref();
        let min_size: usize = if prefix.is_some() {
            if suffix.is_some() { 7 } else { 6 }
        } else {
            5
        };

        let start_at = if prefix.is_some() { 1 } else { 0 };
        let v = src.from_base58().map_err(|err| match err {
            Error::InvalidBase58Character(ch, _) => Self::Error::InvalidChar(ch),
            Error::InvalidBase58Length => Self::Error::InvalidLength,
        })?;

        let s = v.as_slice();
        if s.len() < min_size {
            return Err(Self::Error::InvalidLength);
        }

        let sha = (&s[..s.len() - 4]).sha256_twice();
        if sha[..4] != s[s.len() - 4..] {
            return Err(Self::Error::InvalidChecksum);
        }

        if let Some(prefix) = prefix {
            if prefix != s[0] {
                return Err(Self::Error::InvalidPrefix(prefix, s[0]));
            }
        }

        if let Some(suffix) = suffix {
            let last = s[s.len() - 5];
            if last != suffix {
                Err(Self::Error::InvalidSuffix(suffix, last))
            } else {
                Ok(s[start_at..s.len() - 5].to_vec())
            }
        } else {
            Ok(s[start_at..s.len() - 4].to_vec())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encoding::hex::ToHex;

    #[test]
    fn test_base58_check() {
        let origin = "1BpEi6DfDAUFd7GtittLSdBeYJvcoaVggu";
        let decoded =
            Vec::from_base58_check(&origin, Some(0x00), None).expect("decode should be ok");

        let encoded = Vec::to_base58_check(&decoded, Some(0x00), None);
        assert_eq!(origin, encoded);

        let origin = "1234567890";
        let _ = Vec::from_base58_check(origin, None, None).expect_err("decode should be failed");

        let encoded = origin.to_base58_check(None, Some(0x01));
        let decoded =
            Vec::from_base58_check(&encoded, None, Some(0x01)).expect("decode should be ok");
        assert_eq!(origin.as_bytes(), decoded.as_slice());

        let encoded = origin.to_base58_check(Some(0x03), None);
        let _ = Vec::from_base58_check((encoded + "x").as_str(), Some(0x03), None)
            .expect_err("decode should be failed");
    }

    #[test]
    fn test_base58_addr() {
        let addr = "AceQbAj2xuFLiH5hQAHMnV39wtmjUKiVRj";
        let addr = Vec::from_base58_check(addr, None, None).expect("decode should be ok");

        assert_eq!(addr.to_hex(), "17e4f124b1c3b23553f07cebfb852b2a60aa6c6d94");
    }
}
