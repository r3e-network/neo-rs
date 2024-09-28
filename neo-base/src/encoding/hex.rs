// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};

pub use hex::{FromHex, FromHexError};

pub trait ToHex {
    fn to_hex(&self) -> String;

    fn to_hex_upper(&self) -> String;
}

impl<T: AsRef<[u8]>> ToHex for T {
    #[inline]
    fn to_hex(&self) -> String {
        hex::encode(self)
    }

    #[inline]
    fn to_hex_upper(&self) -> String {
        hex::encode_upper(self)
    }
}

pub trait ToRevHex {
    fn to_rev_hex(&self) -> String;

    fn to_rev_hex_upper(&self) -> String;
}

#[inline]
fn encode_hex(data: &[u8], table: &[u8; 16]) -> String {
    let mut h = String::with_capacity(data.len());
    data.iter().rev().for_each(|b| {
        h.push(table[(b >> 4) as usize] as char);
        h.push(table[(b & 0x0F) as usize] as char);
    });

    h
}

impl<T: AsRef<[u8]>> ToRevHex for T {
    #[inline]
    fn to_rev_hex(&self) -> String {
        encode_hex(self.as_ref(), b"0123456789abcdef")
    }

    #[inline]
    fn to_rev_hex_upper(&self) -> String {
        encode_hex(self.as_ref(), b"0123456789ABCDEF")
    }
}

pub trait DecodeHex {
    type Error;

    fn decode_hex(&self) -> Result<Vec<u8>, Self::Error>;
}

impl<T: AsRef<[u8]>> DecodeHex for T {
    type Error = FromHexError;

    #[inline]
    fn decode_hex(&self) -> Result<Vec<u8>, Self::Error> {
        Vec::from_hex(self)
    }
}

// big-endian hex-encoded
pub trait FromRevHex: Sized {
    type Error;

    fn from_rev_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error>;
}

impl FromRevHex for Vec<u8> {
    type Error = FromHexError;

    fn from_rev_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        let hex = hex.as_ref();
        let hex = if hex.starts_with_0x() { &hex[2..] } else { hex };

        let mut out = Vec::from_hex(hex.as_ref())?;
        out.reverse();

        Ok(out)
    }
}

impl<const N: usize> FromRevHex for [u8; N] {
    type Error = FromHexError;

    fn from_rev_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        let hex = hex.as_ref();
        let hex = if hex.starts_with_0x() { &hex[2..] } else { hex };

        let mut out = [0u8; N];
        hex::decode_to_slice(hex, out.as_mut_slice())?;

        out.reverse();
        Ok(out)
    }
}

pub trait StartsWith0x {
    fn starts_with_0x(&self) -> bool;
}

impl<T: AsRef<[u8]>> StartsWith0x for T {
    #[inline]
    fn starts_with_0x(&self) -> bool {
        let v = self.as_ref();
        v.starts_with("0x".as_bytes()) || v.starts_with("0X".as_bytes())
    }
}

#[cfg(test)]
mod test {
    use super::FromRevHex;
    use crate::encoding::base64::ToBase64;

    #[test]
    fn test_from_hex_be() {
        let script = <[u8; 20]>::from_rev_hex("0xed7cc6f5f2dd842d384f254bc0c2d58fb69a4761")
            .expect("decode should be ok");

        assert_eq!(script.to_base64_std(), "YUeato/VwsBLJU84LYTd8vXGfO0=");
    }
}
