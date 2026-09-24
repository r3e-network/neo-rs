use crate::{IoResult, serializable::Serializable, var_int};
use bytes::{BufMut, Bytes};

/// A sequential binary writer for serializing Neo protocol data in little-endian format.
///
/// Wraps an internal `Vec<u8>` buffer and provides typed write methods
/// matching the Neo C# `BinaryWriter` interface.
///
/// # Zero-Copy Optimization
///
/// For performance-critical paths, use [`with_capacity`](Self::with_capacity) when the
/// output size is known to avoid reallocations. The writer supports zero-copy reads
/// via [`as_bytes`](Self::as_bytes) and can be converted to reference-counted
/// [`Bytes`] via [`freeze`](Self::freeze) for efficient sharing across threads or
/// network layers without cloning the underlying buffer.
#[derive(Debug, Clone, Default)]
pub struct BinaryWriter {
    buffer: Vec<u8>,
}

impl BinaryWriter {
    /// Creates a new empty binary writer.
    #[must_use]
    pub const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Creates a new binary writer with the specified initial capacity.
    ///
    /// # Arguments
    /// * `capacity` - The initial capacity to allocate for the internal buffer.
    ///
    /// # Optimization
    /// Using this constructor when the expected size of the serialized data is known
    /// upfront avoids repeated reallocations as the buffer grows, improving performance
    /// for large or predictable serialization tasks.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Returns the number of bytes written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true when the writer contains zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Writes a single unsigned byte.
    #[inline]
    pub fn write_u8(&mut self, value: u8) -> IoResult<()> {
        self.buffer.push(value);
        Ok(())
    }

    /// Alias for [`write_u8`](Self::write_u8).
    #[inline]
    pub fn write_byte(&mut self, value: u8) -> IoResult<()> {
        self.write_u8(value)
    }

    /// Writes a boolean as a single byte (`0x01` for true, `0x00` for false).
    pub fn write_bool(&mut self, value: bool) -> IoResult<()> {
        self.write_u8(u8::from(value))
    }

    /// Writes a `u16` in little-endian byte order.
    pub fn write_u16(&mut self, value: u16) -> IoResult<()> {
        self.buffer.put_u16_le(value);
        Ok(())
    }

    /// Writes an `i16` in little-endian byte order.
    pub fn write_i16(&mut self, value: i16) -> IoResult<()> {
        self.buffer.put_i16_le(value);
        Ok(())
    }

    /// Writes a `u32` in little-endian byte order.
    pub fn write_u32(&mut self, value: u32) -> IoResult<()> {
        self.buffer.put_u32_le(value);
        Ok(())
    }

    /// Writes an `i32` in little-endian byte order.
    pub fn write_i32(&mut self, value: i32) -> IoResult<()> {
        self.buffer.put_i32_le(value);
        Ok(())
    }

    /// Writes an `i64` in little-endian byte order.
    pub fn write_i64(&mut self, value: i64) -> IoResult<()> {
        self.buffer.put_i64_le(value);
        Ok(())
    }

    /// Writes a `u64` in little-endian byte order.
    pub fn write_u64(&mut self, value: u64) -> IoResult<()> {
        self.buffer.put_u64_le(value);
        Ok(())
    }

    /// Writes a raw byte slice to the buffer.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> IoResult<()> {
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    /// Writes a variable-length integer using the Neo compact encoding.
    ///
    /// Values below 0xFD are stored as a single byte; larger values use a
    /// 2-, 4-, or 8-byte little-endian representation prefixed by 0xFD/0xFE/0xFF.
    pub fn write_var_int(&mut self, value: u64) -> IoResult<()> {
        var_int::write_var_int(value, &mut self.buffer);
        Ok(())
    }

    /// Alias for [`write_var_int`](Self::write_var_int).
    #[inline]
    pub fn write_var_uint(&mut self, value: u64) -> IoResult<()> {
        self.write_var_int(value)
    }

    /// Writes a length-prefixed byte slice (variable-length integer prefix followed by raw bytes).
    pub fn write_var_bytes(&mut self, bytes: &[u8]) -> IoResult<()> {
        var_int::write_var_bytes(bytes, &mut self.buffer);
        Ok(())
    }

    /// Writes a UTF-8 string as length-prefixed bytes.
    pub fn write_var_string(&mut self, value: &str) -> IoResult<()> {
        self.write_var_bytes(value.as_bytes())
    }

    /// Returns a reference to the internal buffer without cloning (zero-copy).
    ///
    /// Use this when you only need to read the serialized bytes without taking ownership.
    /// For an owned copy, use `to_bytes()`; to take ownership, use `into_bytes()`.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// Returns a clone of the internal buffer.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.buffer.clone()
    }

    /// Consumes the writer and returns the internal buffer.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Consumes the writer and returns a reference-counted [`Bytes`] buffer.
    ///
    /// # Zero-Copy Optimization
    ///
    /// This method provides efficient zero-copy sharing of the serialized data.
    /// The returned `Bytes` can be cloned cheaply (only increments a reference count)
    /// making it ideal for:
    /// - Sharing serialized data across multiple threads
    /// - Sending the same data over multiple network connections
    /// - Caching serialized payloads without duplication
    ///
    /// # Example
    ///
    /// ```ignore
    /// let writer = BinaryWriter::new();
    /// // ... serialize data ...
    /// let bytes = writer.freeze();
    /// let clone1 = bytes.clone(); // Zero-copy clone
    /// let clone2 = bytes.clone(); // Another zero-copy clone
    /// ```
    #[must_use]
    pub fn freeze(self) -> Bytes {
        Bytes::from(self.buffer)
    }

    /// Reserves capacity for at least `additional` more bytes in the buffer.
    ///
    /// # Zero-Copy Optimization
    ///
    /// Use this when you know approximately how much more data will be written
    /// to avoid multiple reallocations during serialization.
    pub fn reserve(&mut self, additional: usize) {
        self.buffer.reserve(additional);
    }

    /// Serializes a single [`Serializable`] value into the buffer.
    pub fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()> {
        value.serialize(self)
    }

    /// Writes a length-prefixed vector of [`Serializable`] values.
    pub fn write_serializable_vec<T: Serializable>(&mut self, values: &[T]) -> IoResult<()> {
        self.write_var_uint(values.len() as u64)?;
        for value in values {
            value.serialize(self)?;
        }
        Ok(())
    }

    /// Writes a length-prefixed vector of [`Serializable`] values with capacity pre-allocation.
    ///
    /// # Zero-Copy Optimization
    ///
    /// This variant pre-calculates the total size needed and reserves capacity upfront,
    /// avoiding reallocations during serialization. Use this for performance-critical
    /// paths where the collection size is non-trivial.
    pub fn write_serializable_vec_with_size<T: Serializable>(
        &mut self,
        values: &[T],
    ) -> IoResult<()> {
        // Pre-calculate total size to avoid reallocations
        let prefix_size = var_int::encoded_len(values.len() as u64);
        let items_size: usize = values.iter().map(|v| v.size()).sum();
        self.reserve(prefix_size + items_size);

        // Write the data
        self.write_var_uint(values.len() as u64)?;
        for value in values {
            value.serialize(self)?;
        }
        Ok(())
    }
}
