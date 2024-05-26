// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;

pub use hex::{FromHex, FromHexError};

pub trait ToHex {
    fn to_hex_lower(&self) -> String;

    fn to_hex_upper(&self) -> String;
}

impl<T: AsRef<[u8]>> ToHex for T {
    #[inline]
    fn to_hex_lower(&self) -> String {
        hex::encode(self)
    }

    #[inline]
    fn to_hex_upper(&self) -> String {
        hex::encode_upper(self)
    }
}

pub trait ToRevHex {
    fn to_rev_hex_lower(&self) -> String;

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
    fn to_rev_hex_lower(&self) -> String {
        encode_hex(self.as_ref(), b"0123456789abcdef")
    }

    #[inline]
    fn to_rev_hex_upper(&self) -> String {
        encode_hex(self.as_ref(), b"0123456789ABCDEF")
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
