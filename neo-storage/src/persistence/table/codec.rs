//! Allocation-aware codecs used by typed logical tables.

use std::borrow::Cow;

use crate::{StorageError, StorageItem, StorageKey, StorageResult};

/// Encoded table bytes that can be persisted without an avoidable second copy.
pub trait IntoTableBytes: AsRef<[u8]> {
    /// Converts the encoded representation into owned storage bytes.
    fn into_table_bytes(self) -> Vec<u8>;
}

impl IntoTableBytes for Vec<u8> {
    #[inline]
    fn into_table_bytes(self) -> Vec<u8> {
        self
    }
}

impl<const N: usize> IntoTableBytes for [u8; N] {
    #[inline]
    fn into_table_bytes(self) -> Vec<u8> {
        self.to_vec()
    }
}

impl IntoTableBytes for &[u8] {
    #[inline]
    fn into_table_bytes(self) -> Vec<u8> {
        self.to_vec()
    }
}

impl IntoTableBytes for Cow<'_, [u8]> {
    #[inline]
    fn into_table_bytes(self) -> Vec<u8> {
        self.into_owned()
    }
}

/// Encodes one typed table key or value.
///
/// The GAT lets codecs return borrowed slices, fixed-size stack arrays, or an
/// owned vector without forcing one representation on every storage domain.
pub trait TableEncode<T: ?Sized> {
    /// Concrete encoded representation for one borrowed input.
    type Encoded<'a>: IntoTableBytes
    where
        T: 'a;

    /// Encodes `value` using the table's stable persisted representation.
    fn encode<'a>(value: &'a T) -> StorageResult<Self::Encoded<'a>>;
}

/// Decodes one typed table key or value from persisted bytes.
pub trait TableDecode<T> {
    /// Decodes and validates one persisted value.
    fn decode(bytes: &[u8]) -> StorageResult<T>;
}

/// Complete table value codec supporting both persistence and retrieval.
pub trait TableCodec<T>: TableEncode<T> + TableDecode<T> {}

impl<T, C> TableCodec<T> for C where C: TableEncode<T> + TableDecode<T> {}

/// Pass-through codec for owned byte vectors.
#[derive(Debug)]
pub struct BytesCodec;

impl TableEncode<Vec<u8>> for BytesCodec {
    type Encoded<'a> = &'a [u8];

    #[inline]
    fn encode(value: &Vec<u8>) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.as_slice())
    }
}

impl TableDecode<Vec<u8>> for BytesCodec {
    #[inline]
    fn decode(bytes: &[u8]) -> StorageResult<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}

/// Exact-length codec for fixed byte arrays.
#[derive(Debug)]
pub struct FixedBytesCodec<const N: usize>;

impl<const N: usize> TableEncode<[u8; N]> for FixedBytesCodec<N> {
    type Encoded<'a> = &'a [u8];

    #[inline]
    fn encode(value: &[u8; N]) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.as_slice())
    }
}

impl<const N: usize> TableDecode<[u8; N]> for FixedBytesCodec<N> {
    fn decode(bytes: &[u8]) -> StorageResult<[u8; N]> {
        bytes.try_into().map_err(|_| {
            StorageError::invalid_data(format!("expected {N} fixed bytes, found {}", bytes.len()))
        })
    }
}

/// Big-endian codec whose lexicographic order matches `u32` numeric order.
#[derive(Debug)]
pub struct U32BeCodec;

impl TableEncode<u32> for U32BeCodec {
    type Encoded<'a> = [u8; 4];

    #[inline]
    fn encode(value: &u32) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.to_be_bytes())
    }
}

impl TableDecode<u32> for U32BeCodec {
    fn decode(bytes: &[u8]) -> StorageResult<u32> {
        let encoded: [u8; 4] = bytes.try_into().map_err(|_| {
            StorageError::invalid_data(format!(
                "expected 4-byte big-endian u32, found {} bytes",
                bytes.len()
            ))
        })?;
        Ok(u32::from_be_bytes(encoded))
    }
}

/// Big-endian codec whose lexicographic order matches `u64` numeric order.
#[derive(Debug)]
pub struct U64BeCodec;

impl TableEncode<u64> for U64BeCodec {
    type Encoded<'a> = [u8; 8];

    #[inline]
    fn encode(value: &u64) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.to_be_bytes())
    }
}

impl TableDecode<u64> for U64BeCodec {
    fn decode(bytes: &[u8]) -> StorageResult<u64> {
        let encoded: [u8; 8] = bytes.try_into().map_err(|_| {
            StorageError::invalid_data(format!(
                "expected 8-byte big-endian u64, found {} bytes",
                bytes.len()
            ))
        })?;
        Ok(u64::from_be_bytes(encoded))
    }
}

/// Byte-identical codec for Neo contract-storage keys.
#[derive(Debug)]
pub struct StorageKeyCodec;

impl TableEncode<StorageKey> for StorageKeyCodec {
    type Encoded<'a> = Cow<'a, [u8]>;

    #[inline]
    fn encode(value: &StorageKey) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.as_bytes())
    }
}

impl TableDecode<StorageKey> for StorageKeyCodec {
    fn decode(bytes: &[u8]) -> StorageResult<StorageKey> {
        if bytes.len() < std::mem::size_of::<i32>() {
            return Err(StorageError::invalid_data(format!(
                "Neo storage key must include a 4-byte contract id, found {} bytes",
                bytes.len()
            )));
        }
        Ok(StorageKey::from_bytes(bytes))
    }
}

/// Byte-identical codec for Neo contract-storage values.
#[derive(Debug)]
pub struct StorageItemCodec;

impl TableEncode<StorageItem> for StorageItemCodec {
    type Encoded<'a> = Cow<'a, [u8]>;

    #[inline]
    fn encode(value: &StorageItem) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.value_bytes())
    }
}

impl TableDecode<StorageItem> for StorageItemCodec {
    #[inline]
    fn decode(bytes: &[u8]) -> StorageResult<StorageItem> {
        Ok(StorageItem::from_bytes(bytes.to_vec()))
    }
}
