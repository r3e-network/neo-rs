// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{string::String, vec::Vec};
use bytes::{BytesMut, BufMut, Bytes, Buf};

use crate::{errors, hash::{Sha256, SHA256_HASH_SIZE}};

pub use neo_proc_macros::{BinEncode, BinDecode, InnerBinDecode};


pub trait BinWriter {
    fn write_varint_le(&mut self, value: u64);

    fn write<T: AsRef<[u8]>>(&mut self, value: T);

    fn len(&self) -> usize;
}


impl BinWriter for BytesMut {
    fn write_varint_le(&mut self, value: u64) {
        let (size, buf) = to_varint_le(value);
        self.put_slice(&buf[..size as usize]); // size field
    }

    fn write<T: AsRef<[u8]>>(&mut self, value: T) {
        self.put_slice(value.as_ref());
    }

    fn len(&self) -> usize { self.len() }
}


pub trait BinReader {
    fn read_varint_le(&mut self) -> Result<u64, BinDecodeError> {
        let mut buf = [0u8; 1];
        self.read_full(buf.as_mut_slice())?;

        match buf[0] {
            0xfd => {
                let mut buf = [0u8; 2];
                self.read_full(buf.as_mut_slice())?;
                Ok(u16::from_le_bytes(buf) as u64)
            }
            0xfe => {
                let mut buf = [0u8; 4];
                self.read_full(buf.as_mut_slice())?;
                Ok(u32::from_le_bytes(buf) as u64)
            }
            0xff => {
                let mut buf = [0u8; 8];
                self.read_full(buf.as_mut_slice())?;
                Ok(u64::from_le_bytes(buf))
            }
            n => Ok(n as u64) // just one byte
        }
    }

    /// like `read_exact`
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), BinDecodeError>;

    fn consumed(&self) -> usize;

    fn remaining(&self) -> usize;

    fn discard(&mut self, n: usize) -> usize;
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum BinDecodeError {
    #[error("bin-decode: end of the buffer")]
    EndOfBuffer,

    #[error("bin-decode: invalid value of '{0}' start at offset {1}")]
    InvalidValue(&'static str, usize),

    #[error("bin-decode: invalid type field '{2}' of '{0}' start at offset {1}")]
    InvalidType(&'static str, usize, u64),

    #[error("bin-decode: invalid length of '{1}' in '{0}'({2})")]
    InvalidLength(&'static str, &'static str, usize),
}

pub struct Buffer {
    consumed: usize,
    buf: Bytes,
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] { self.buf.as_ref() }
}

impl From<Bytes> for Buffer {
    fn from(value: Bytes) -> Self { Self { consumed: 0, buf: value } }
}

impl From<BytesMut> for Buffer {
    fn from(value: BytesMut) -> Self { Self { consumed: 0, buf: Bytes::from(value) } }
}

impl From<Vec<u8>> for Buffer {
    fn from(value: Vec<u8>) -> Self { Self { consumed: 0, buf: Bytes::from(value) } }
}

impl BinReader for Buffer {
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), BinDecodeError> {
        if self.remaining() < buf.len() {
            return Err(BinDecodeError::EndOfBuffer);
        }

        self.buf.copy_to_slice(buf);
        self.consumed += buf.len();
        Ok(())
    }

    fn consumed(&self) -> usize { self.consumed }

    fn remaining(&self) -> usize { self.buf.remaining() }

    fn discard(&mut self, n: usize) -> usize {
        let n = core::cmp::min(n, self.remaining());
        self.buf.advance(n);
        self.consumed += n;
        n
    }
}


pub struct RefBuffer<'a> {
    consumed: usize,
    buf: &'a [u8],
}

impl<'a> RefBuffer<'a> {
    pub fn as_bytes(&self) -> &[u8] { &self.buf[self.consumed..] }
}

impl<'a> AsRef<[u8]> for RefBuffer<'a> {
    fn as_ref(&self) -> &[u8] { &self.buf[self.consumed..] }
}

impl<'a> From<&'a [u8]> for RefBuffer<'a> {
    fn from(value: &'a [u8]) -> Self { Self { consumed: 0, buf: value } }
}

impl<'a> BinReader for RefBuffer<'a> {
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), BinDecodeError> {
        if self.remaining() < buf.len() {
            return Err(BinDecodeError::EndOfBuffer);
        }

        let remain = &self.buf[self.consumed..];
        let n = core::cmp::min(remain.len(), buf.len());

        buf[..n].copy_from_slice(&remain[..n]);
        self.consumed += n;
        Ok(())
    }

    fn consumed(&self) -> usize { self.consumed }

    fn remaining(&self) -> usize { self.buf.len() - self.consumed }

    fn discard(&mut self, n: usize) -> usize {
        let n = core::cmp::min(n, self.remaining());
        self.consumed += n;
        n
    }
}


pub trait BinEncoder {
    fn encode_bin(&self, w: &mut impl BinWriter);

    fn bin_size(&self) -> usize;
}


pub trait BinDecoder: Sized {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError>;
}


impl BinEncoder for bool {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(if *self { [1u8] } else { [0u8] });
    }

    fn bin_size(&self) -> usize { core::mem::size_of::<Self>() }
}

impl BinEncoder for u8 {
    fn encode_bin(&self, w: &mut impl BinWriter) { w.write(&self.to_le_bytes()); }

    fn bin_size(&self) -> usize { core::mem::size_of::<Self>() }
}

impl BinEncoder for u16 {
    fn encode_bin(&self, w: &mut impl BinWriter) { w.write(&self.to_le_bytes()); }

    fn bin_size(&self) -> usize { core::mem::size_of::<Self>() }
}

impl BinEncoder for u32 {
    fn encode_bin(&self, w: &mut impl BinWriter) { w.write(&self.to_le_bytes()); }

    fn bin_size(&self) -> usize { core::mem::size_of::<Self>() }
}

impl BinEncoder for u64 {
    fn encode_bin(&self, w: &mut impl BinWriter) { w.write(&self.to_le_bytes()); }

    fn bin_size(&self) -> usize { core::mem::size_of::<Self>() }
}

impl BinEncoder for String {
    fn encode_bin(&self, w: &mut impl BinWriter) { self.as_str().encode_bin(w); }

    fn bin_size(&self) -> usize { self.as_str().bin_size() }
}

impl BinEncoder for str {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write_varint_le(self.as_bytes().len() as u64);
        w.write(self.as_bytes());
    }

    fn bin_size(&self) -> usize {
        let (size, _) = to_varint_le(self.as_bytes().len() as u64);
        size as usize + self.as_bytes().len()
    }
}

impl<T: BinEncoder> BinEncoder for [T] {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write_varint_le(self.len() as u64);
        self.iter().for_each(|it| it.encode_bin(w))
    }

    fn bin_size(&self) -> usize {
        let (size, _) = to_varint_le(self.len() as u64);
        size as usize + self.iter().map(|it| it.bin_size()).sum::<usize>()
    }
}

impl<T: BinEncoder> BinEncoder for Vec<T> {
    fn encode_bin(&self, w: &mut impl BinWriter) { self.as_slice().encode_bin(w) }

    fn bin_size(&self) -> usize {
        let (size, _) = to_varint_le(self.len() as u64);
        size as usize + self.iter().map(|it| it.bin_size()).sum::<usize>()
    }
}

impl BinEncoder for () {
    fn encode_bin(&self, _w: &mut impl BinWriter) {}

    fn bin_size(&self) -> usize { 0 }
}

impl<T1: BinEncoder, T2: BinEncoder> BinEncoder for (&T1, &T2) {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        self.0.encode_bin(w);
        self.1.encode_bin(w);
    }

    fn bin_size(&self) -> usize { self.0.bin_size() + self.1.bin_size() }
}

impl<T1: BinEncoder, T2: BinEncoder, T3: BinEncoder> BinEncoder for (&T1, &T2, &T3) {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        self.0.encode_bin(w);
        self.1.encode_bin(w);
        self.2.encode_bin(w);
    }

    fn bin_size(&self) -> usize { self.0.bin_size() + self.1.bin_size() + self.2.bin_size() }
}


pub trait ToBinEncoded {
    fn to_bin_encoded(&self) -> Vec<u8>;
}

impl<T: BinEncoder> ToBinEncoded for T {
    #[inline]
    fn to_bin_encoded(&self) -> Vec<u8> {
        let mut w = BytesMut::with_capacity(self.bin_size());
        self.encode_bin(&mut w);

        w.into()
    }
}


/// Bin-encoding and then computing the SHA256
pub trait BinSha256 {
    fn bin_sha256(&self) -> [u8; SHA256_HASH_SIZE];
}

impl<T: BinEncoder> BinSha256 for T {
    #[inline]
    fn bin_sha256(&self) -> [u8; SHA256_HASH_SIZE] { self.to_bin_encoded().sha256() }
}


impl BinDecoder for bool {
    fn decode_bin(r: &mut impl BinReader) -> Result<bool, BinDecodeError> {
        let offset = r.consumed();
        let b = u8::decode_bin(r)?;
        if b != 0x0 && b != 0x1 {
            Err(BinDecodeError::InvalidValue("bool", offset))
        } else {
            Ok(b == 0x01)
        }
    }
}

impl BinDecoder for u8 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u8, BinDecodeError> {
        let mut buf = [0u8; 1];
        r.read_full(buf.as_mut_slice())?;
        Ok(buf[0])
    }
}

impl BinDecoder for u16 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u16, BinDecodeError> {
        let mut buf = [0u8; 2];
        r.read_full(buf.as_mut_slice())?;
        Ok(u16::from_le_bytes(buf))
    }
}

impl BinDecoder for u32 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u32, BinDecodeError> {
        let mut buf = [0u8; 4];
        r.read_full(buf.as_mut_slice())?;
        Ok(u32::from_le_bytes(buf))
    }
}

impl BinDecoder for u64 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u64, BinDecodeError> {
        let mut buf = [0u8; 8];
        r.read_full(buf.as_mut_slice())?;
        Ok(u64::from_le_bytes(buf))
    }
}

impl BinDecoder for String {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let offset = r.consumed();
        String::from_utf8(Vec::decode_bin(r)?)
            .map_err(|_err| BinDecodeError::InvalidValue("String", offset))
    }
}

impl<T: BinDecoder> BinDecoder for Vec<T> {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let size = r.read_varint_le()? as usize;
        let each = core::mem::size_of::<T>();
        let cap = if each > 0 { 31 / each + 1 } else { 0 };

        let mut items = Vec::with_capacity(core::cmp::min(size, cap));
        for _i in 0..size {
            items.push(T::decode_bin(r)?);
        }

        Ok(items)
    }
}

impl BinDecoder for () {
    fn decode_bin(_r: &mut impl BinReader) -> Result<Self, BinDecodeError> { Ok(()) }
}

impl<T1: BinDecoder, T2: BinDecoder> BinDecoder for (T1, T2) {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        Ok((BinDecoder::decode_bin(r)?, BinDecoder::decode_bin(r)?))
    }
}

impl<T1: BinDecoder, T2: BinDecoder, T3: BinDecoder> BinDecoder for (T1, T2, T3) {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        Ok((BinDecoder::decode_bin(r)?, BinDecoder::decode_bin(r)?, BinDecoder::decode_bin(r)?))
    }
}


pub trait EncodeHashFields {
    fn encode_hash_fields(&self, w: &mut impl BinWriter);
}

pub trait HashFieldsSha256 {
    fn hash_fields_sha256(&self) -> [u8; SHA256_HASH_SIZE];
}

impl<T: EncodeHashFields> HashFieldsSha256 for T {
    fn hash_fields_sha256(&self) -> [u8; SHA256_HASH_SIZE] {
        let mut w = BytesMut::with_capacity(256);
        self.encode_hash_fields(&mut w);
        w.sha256()
    }
}


pub fn to_varint_le(value: u64) -> (u8, [u8; 9]) {
    let mut le = [0u8; 9];
    if value < 0xfd {
        le[0] = value as u8;
        (1, le)
    } else if value < 0xFFFF {
        le[0] = 0xfd;
        le[1..=2].copy_from_slice(&(value as u16).to_le_bytes());
        (3, le)
    } else if value < 0xFFFFFFFF {
        le[0] = 0xfe;
        le[1..=4].copy_from_slice(&(value as u32).to_le_bytes());
        (5, le)
    } else {
        le[0] = 0xff;
        le[1..=8].copy_from_slice(&value.to_le_bytes());
        (9, le)
    }
}


pub mod big_endian {
    use super::*;

    #[derive(Debug, Copy, Clone)]
    pub struct U32(pub u32);

    impl U32 {
        pub fn as_u32(&self) -> u32 { self.0 }
    }

    impl BinEncoder for U32 {
        fn encode_bin(&self, w: &mut impl BinWriter) { w.write(&self.0.to_be_bytes()) }

        fn bin_size(&self) -> usize { core::mem::size_of::<u32>() }
    }

    impl BinDecoder for U32 {
        fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
            BinDecoder::decode_bin(r).map(|v: u32| Self(v.swap_bytes()))
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use alloc::vec;
    use crate::encoding::hex::ToHex;

    #[test]
    fn test_write_le() {
        let mut w = BytesMut::new();

        w.write(0x1u8.to_le_bytes());
        assert_eq!(w.len(), 1);

        w.write(0x2233u16.to_le_bytes());
        assert_eq!(w.len(), 3);
        assert_eq!(&[0x1u8, 0x33, 0x22], w.as_ref());

        [0x44u8, 0x55, 0x66, 0x77].as_slice().encode_bin(&mut w);
        assert_eq!(w.len(), 8); // size + items
    }

    #[derive(Debug, BinEncode, BinDecode, PartialEq, Eq)]
    pub enum BinEnum {
        X = 0x01,
        Y = 0x02,
    }

    #[derive(Debug, BinEncode, BinDecode, PartialEq, Eq)]
    #[bin(repr = u16)]
    pub enum BinRepr {
        X = 0x01,
    }

    #[test]
    fn test_bin_encode_enum() {
        let mut w = BytesMut::with_capacity(128);
        BinEnum::X.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "01");

        let mut r = Buffer::from(w);
        let x: BinEnum = BinDecoder::decode_bin(&mut r).expect("decode should be ok");
        assert_eq!(x, BinEnum::X);
        assert_eq!(r.remaining(), 0usize);

        let mut w = BytesMut::with_capacity(128);
        BinEnum::Y.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "02");

        let mut w = BytesMut::with_capacity(128);
        BinRepr::X.encode_bin(&mut w);

        let _ = <BinEnum as BinDecoder>::decode_bin(&mut Buffer::from(vec![0x03u8]))
            .expect_err("decode should be fail");

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "0100");

        let mut r = Buffer::from(w);
        let x: BinRepr = BinDecoder::decode_bin(&mut r).expect("decode should be ok");
        assert_eq!(x, BinRepr::X);
        assert_eq!(r.remaining(), 0usize);
    }


    #[derive(Debug, BinEncode, BinDecode, PartialEq, Eq)]
    #[bin(repr = u8)]
    pub enum BinMatch {
        #[bin(tag = 0x00)] X { x: u64, x2: u32 },
        #[bin(tag = 0x01)] Y(u32, #[bin(ignore)] u64),
        #[bin(tag = 0x02)] Z,
        #[bin(tag = 0x03)] A {},
    }

    #[test]
    fn test_enum_match() {
        let m = BinMatch::X { x: 0x01020304, x2: 0x11223344 };
        let mut w = BytesMut::with_capacity(128);
        m.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "00040302010000000044332211");

        let mut r = Buffer::from(w);
        let x: BinMatch = BinDecoder::decode_bin(&mut r).expect("decode should be ok");
        assert_eq!(x, BinMatch::X { x: 0x01020304, x2: 0x11223344 });
        assert_eq!(r.remaining(), 0usize);

        let m = BinMatch::A {};
        let mut w = BytesMut::with_capacity(128);
        m.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "03");

        let m = BinMatch::Z;
        let mut w = BytesMut::with_capacity(128);
        m.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "02");

        let m = BinMatch::Y(0x01020304, 0);
        let mut w = BytesMut::with_capacity(128);
        m.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "0104030201");
    }

    #[derive(Debug, BinEncode, BinDecode, PartialEq, Eq)]
    struct Bin {
        x: u64,
        y: u32,
        z: String,

        #[bin(ignore)]
        hash: u64,
    }

    #[derive(Debug, BinEncode, BinDecode, PartialEq, Eq)]
    struct BinUnnamed(u64, u32, String, #[bin(ignore)]u64);

    #[test]
    fn test_bin_encode_struct() {
        let b = Bin {
            x: 0x1122334455667788,
            y: 0x01020304,
            z: "Hello world".into(),
            hash: 0x90919203,
        };

        let mut w = BytesMut::with_capacity(128);
        b.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "8877665544332211040302010b48656c6c6f20776f726c64");
        assert_eq!(b.hash, 0x90919203);

        let mut r = Buffer::from(w);
        let k: Bin = BinDecoder::decode_bin(&mut r).expect("decode should be ok");
        assert_eq!(k.x, b.x);
        assert_eq!(k.y, b.y);
        assert_eq!(k.z, b.z);

        let b = BinUnnamed(
            0x1122334455667788,
            0x01020304,
            "Hello world".into(),
            0x90919203,
        );

        let mut w = BytesMut::with_capacity(128);
        b.encode_bin(&mut w);

        let x = w.as_ref().to_hex();
        assert_eq!(&x, "8877665544332211040302010b48656c6c6f20776f726c64");
        assert_eq!(b.3, 0x90919203);

        let mut r = Buffer::from(w);
        let k: BinUnnamed = BinDecoder::decode_bin(&mut r).expect("decode should be ok");
        assert_eq!(k.0, b.0);
        assert_eq!(k.1, b.1);
        assert_eq!(k.2, b.2);
    }
}