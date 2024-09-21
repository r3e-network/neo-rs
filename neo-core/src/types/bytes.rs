// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec, vec::Vec};
use core::net::IpAddr;

use bytes::{BufMut, BytesMut};
use neo_base::encoding::{base64::*, bin::*};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FixedBytes<const N: usize>(pub(crate) [u8; N]);

impl<const N: usize> FixedBytes<N> {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }

    pub fn is_zero(&self) -> bool { self.0.iter().all(|x| *x == 0) }
}

impl<const N: usize> From<[u8; N]> for FixedBytes<N> {
    fn from(value: [u8; N]) -> Self { Self(value) }
}

impl<const N: usize> From<&[u8]> for FixedBytes<N> {
    fn from(v: &[u8]) -> Self {
        let mut buf = Self([0u8; N]);
        let bound = core::cmp::min(N, v.len());

        buf.0[..bound].copy_from_slice(&v[..bound]);
        buf
    }
}

impl<const N: usize> Into<[u8; N]> for FixedBytes<N> {
    fn into(self) -> [u8; N] { self.0 }
}

impl<const N: usize> AsRef<[u8; N]> for FixedBytes<N> {
    fn as_ref(&self) -> &[u8; N] { &self.0 }
}

impl<const N: usize> AsRef<[u8]> for FixedBytes<N> {
    fn as_ref(&self) -> &[u8] { self.0.as_ref() }
}

impl<const N: usize> Default for FixedBytes<N> {
    fn default() -> Self { Self([0u8; N]) }
}

impl<const N: usize> BinEncoder for FixedBytes<N> {
    fn encode_bin(&self, w: &mut impl BinWriter) { w.write(self.0.as_ref()) }

    fn bin_size(&self) -> usize { N }
}

impl<const N: usize> BinDecoder for FixedBytes<N> {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut buf: FixedBytes<N> = Default::default();
        r.read_full(buf.0.as_mut_slice())?;

        Ok(buf)
    }
}

impl<const N: usize> Serialize for FixedBytes<N> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_base64_std())
    }
}

impl<'de, const N: usize> Deserialize<'de> for FixedBytes<N> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Vec::from_base64_std(value.as_str()).map(|v| v.as_slice().into()).map_err(D::Error::custom)
    }
}

impl From<IpAddr> for FixedBytes<16> {
    #[inline]
    fn from(addr: IpAddr) -> Self {
        match addr {
            IpAddr::V4(addr) => addr.to_ipv6_mapped().octets().into(),
            IpAddr::V6(addr) => addr.octets().into(),
        }
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Bytes(pub(crate) Vec<u8>);

impl Bytes {
    pub fn len(&self) -> usize { self.0.len() }

    pub fn as_bytes(&self) -> &[u8] { self.0.as_slice() }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self { Bytes(value) }
}

impl Into<Vec<u8>> for Bytes {
    fn into(self) -> Vec<u8> { self.0 }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] { self.0.as_ref() }
}

impl Default for Bytes {
    fn default() -> Self { Self(Default::default()) }
}

impl BinEncoder for Bytes {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write_varint_le(self.0.len() as u64);
        w.write(&self.0);
    }

    fn bin_size(&self) -> usize {
        let (size, _) = to_varint_le(self.0.len() as u64);
        size as usize + self.0.len()
    }
}

impl BinDecoder for Bytes {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let size = r.read_varint_le()? as usize;
        if size > r.remaining() {
            return Err(BinDecodeError::EndOfBuffer);
        }

        let mut buf = vec![0u8; size];
        r.read_full(buf.as_mut_slice())?;

        Ok(Bytes(buf))
    }
}

impl Serialize for Bytes {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_base64_std())
    }
}

impl<'de> Deserialize<'de> for Bytes {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Vec::from_base64_std(value.as_str()).map(|v| v.into()).map_err(D::Error::custom)
    }
}

pub(crate) trait Varbytes {
    fn put_varbytes<T: AsRef<[u8]>>(&mut self, bytes: T);
}

impl Varbytes for BytesMut {
    fn put_varbytes<T: AsRef<[u8]>>(&mut self, bytes: T) {
        let bytes = bytes.as_ref();
        if bytes.len() > u32::MAX as usize {
            core::panic!("too many bytes({} > u32::MAX)", bytes.len());
        }

        match bytes.len() {
            0..=0xFF => {
                self.put_u8(0x0C); // PUSH_DATA1
                self.put_u8(bytes.len() as u8);
            }
            0x100..=0xFFFF => {
                self.put_u8(0x0D); // PUSH_DATA2
                self.put_u16_le(bytes.len() as u16);
            }
            _ => {
                self.put_u8(0x0E); // PUSH_DATA4
                self.put_u32_le(bytes.len() as u32);
            }
        }

        self.extend_from_slice(bytes);
    }
}

pub(crate) trait Varint {
    fn put_varint(&mut self, n: u64);
}

impl Varint for BytesMut {
    fn put_varint(&mut self, n: u64) {
        match n {
            0..=16 => {
                // PUSH0
                self.put_u8(0x10 + (n as u8));
            }
            17..=127 => {
                // PUSH8
                self.put_u8(0x00);
                self.put_u8(n as u8);
            }
            128..=32_767 => {
                // PUSH16
                self.put_u8(0x01);
                self.put_u16_le(n as u16);
            }
            32_768..=2_147_483_647 => {
                // PUSH32
                self.put_u8(0x02);
                self.put_u32_le(n as u32);
            }
            2_147_483_648..=9_223_372_036_854_775_807 => {
                // PUSH64
                self.put_u8(0x3);
                self.put_u64_le(n);
            }
            _ => {
                // PUSH128, 9_223_372_036_854_775_808..=18_446_744_073_709_551_615
                self.put_u8(0x04);
                self.put_u128_le(n as u128);
            } // no PUSH256
        }
    }
}
