// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;
use alloc::vec::Vec;

use crate::encoding::{FromBase58Check, ToBase58Check};

#[derive(Debug, Clone)]
pub struct Wif {
    version: u8,

    compressed: bool,

    // include version byte, data-bytes and compressed flag byte
    whole: Vec<u8>,
}

impl Wif {
    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn data(&self) -> &[u8] {
        if self.compressed {
            &self.whole[1..self.whole.len() - 1]
        } else {
            &self.whole[1..]
        }
    }

    pub fn compressed(&self) -> bool {
        self.compressed
    }
}

pub trait WifEncode {
    fn wif_encode(&self, version: u8, compressed: bool) -> String;
}

pub trait WifDecode {
    type Error;

    fn wif_decode(&self, expected_data_size: usize) -> Result<Wif, Self::Error>;
}

impl<T: AsRef<[u8]>> WifEncode for T {
    fn wif_encode(&self, version: u8, compressed: bool) -> String {
        let data = self.as_ref();
        let mut buf = Vec::with_capacity(1 + data.len() + 1);

        buf.push(version);
        buf.extend_from_slice(data);
        if compressed {
            buf.push(1u8);
        }

        buf.to_base58_check()
    }
}

#[derive(Debug, Copy, Clone, thiserror::Error)]
pub enum WifDecodeError {
    #[error("wif-decode: invalid base58 encoded")]
    InvalidBase58Encoded,

    #[error("wif-decode: invalid length '{0}'")]
    InvalidLength(usize),

    #[error("wif-decode: invalid compressed flag")]
    InvalidCompressedFlag(u8),
}

impl<T: AsRef<str>> WifDecode for T {
    type Error = WifDecodeError;

    fn wif_decode(&self, expected_data_size: usize) -> Result<Wif, Self::Error> {
        let b58 = Vec::from_base58_check(self.as_ref())
            .map_err(|_err| Self::Error::InvalidBase58Encoded)?;

        let size = b58.len();
        if size <= 1 || (size != expected_data_size && size != expected_data_size + 1) {
            return Err(Self::Error::InvalidLength(size));
        }

        let compressed = size == expected_data_size + 1;
        let last = b58.last().copied().unwrap_or(0);
        if compressed && last != 0x01 {
            return Err(Self::Error::InvalidCompressedFlag(last));
        }

        Ok(Wif {
            version: b58[0],
            compressed,
            whole: b58,
        })
    }
}
