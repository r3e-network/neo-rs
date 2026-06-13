//! `MemoryReader` - matches C# Neo.IO.MemoryReader exactly

use std::io;
use std::str;

use bytes::Buf;
use thiserror::Error;

use crate::var_int::read_var_int_prefix;

/// Errors that can occur while reading Neo binary data.
#[derive(Debug, Error)]
pub enum IoError {
    /// The stream does not contain enough data or contains malformed data.
    #[error("format error")]
    Format,
    /// The stream contained bytes that are not valid UTF-8.
    #[error("invalid utf-8 data")]
    InvalidUtf8,
    /// The stream contained data that failed a semantic validation.
    #[error("{context}: {value}")]
    InvalidData {
        /// Description of the validation that failed.
        context: String,
        /// The invalid value that was encountered.
        value: String,
    },
    /// Wrapper around lower-level I/O errors when writing.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Result alias for IO operations within Neo.IO.
pub type IoResult<T> = Result<T, IoError>;

macro_rules! read_buf_primitives {
    ($(($name:ident, $value_type:ty, $length:expr, $method:ident);)+) => {
        $(
            #[doc = concat!("Reads the fixed-width primitive for `", stringify!($name), "`.")]
            #[inline]
            pub fn $name(&mut self) -> IoResult<$value_type> {
                self.read_with_buf($length, |bytes| bytes.$method())
            }
        )+
    };
}

/// Memory reader matching C# Neo.IO.MemoryReader.
#[derive(Debug, Clone, Copy)]
pub struct MemoryReader<'a> {
    buffer: &'a [u8],
    position: usize,
}

impl<'a> MemoryReader<'a> {
    /// Creates a new `MemoryReader` (C# constructor)
    #[must_use]
    pub const fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    /// Gets the current position (C# Position property)
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Sets the reader position (C# Position setter)
    pub fn set_position(&mut self, position: usize) -> IoResult<()> {
        if position > self.buffer.len() {
            Err(IoError::Format)
        } else {
            self.position = position;
            Ok(())
        }
    }

    /// Returns the total length of the backing buffer
    #[must_use]
    pub const fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true when the backing buffer contains zero bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the number of unread bytes remaining in the buffer.
    #[inline]
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.position)
    }
    #[inline]
    const fn ensure_available(&self, required: usize) -> IoResult<()> {
        if self.position.saturating_add(required) > self.buffer.len() {
            Err(IoError::Format)
        } else {
            Ok(())
        }
    }

    /// Reads exactly `N` bytes into a fixed-size array.
    #[inline]
    pub fn read_array<const N: usize>(&mut self) -> IoResult<[u8; N]> {
        self.ensure_available(N)?;
        let end = self.position + N;
        let slice = &self.buffer[self.position..end];
        self.position = end;
        slice.try_into().map_err(|_| IoError::Format)
    }

    #[inline]
    fn read_slice(&mut self, length: usize) -> IoResult<&'a [u8]> {
        self.ensure_available(length)?;
        let start = self.position;
        let end = start + length;
        self.position = end;
        Ok(&self.buffer[start..end])
    }

    #[inline]
    fn read_with_buf<T>(
        &mut self,
        length: usize,
        read: impl FnOnce(&mut &[u8]) -> T,
    ) -> IoResult<T> {
        self.ensure_available(length)?;
        let start = self.position;
        let end = start + length;
        let mut bytes = &self.buffer[start..end];
        let value = read(&mut bytes);
        self.position = end;
        Ok(value)
    }

    /// Reads a sequence of bytes and returns them as a borrowed slice (zero-copy).
    ///
    /// This is more efficient than `read_bytes` when you don't need an owned vector.
    #[inline]
    pub fn read_bytes_ref(&mut self, length: usize) -> IoResult<&'a [u8]> {
        self.read_slice(length)
    }

    /// Reads a sequence of bytes and returns them as an owned vector.
    #[inline]
    pub fn read_bytes(&mut self, length: usize) -> IoResult<Vec<u8>> {
        Ok(self.read_slice(length)?.to_vec())
    }

    /// Peeks at the next byte without advancing position (C# Peek)
    #[inline]
    pub fn peek(&self) -> IoResult<u8> {
        self.ensure_available(1)?;
        Ok(self.buffer[self.position])
    }

    /// Reads a boolean (C# `ReadBoolean`)
    #[inline]
    pub fn read_boolean(&mut self) -> IoResult<bool> {
        match self.read_byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(IoError::Format),
        }
    }

    /// Alias for `read_boolean` for API parity with legacy code.
    #[inline]
    pub fn read_bool(&mut self) -> IoResult<bool> {
        self.read_boolean()
    }

    /// Reads a signed byte (C# `ReadSByte`)
    #[inline]
    pub fn read_sbyte(&mut self) -> IoResult<i8> {
        Ok(self.read_byte()? as i8)
    }

    read_buf_primitives! {
        (read_byte, u8, 1, get_u8);
        (read_int16, i16, 2, get_i16_le);
        (read_int16_big_endian, i16, 2, get_i16);
        (read_uint16, u16, 2, get_u16_le);
        (read_uint16_big_endian, u16, 2, get_u16);
        (read_int32, i32, 4, get_i32_le);
        (read_int32_big_endian, i32, 4, get_i32);
        (read_uint32, u32, 4, get_u32_le);
        (read_uint32_big_endian, u32, 4, get_u32);
        (read_int64, i64, 8, get_i64_le);
        (read_int64_big_endian, i64, 8, get_i64);
        (read_uint64, u64, 8, get_u64_le);
        (read_uint64_big_endian, u64, 8, get_u64);
        (read_f32, f32, 4, get_f32_le);
        (read_f64, f64, 8, get_f64_le);
    }

    /// Alias for `read_byte` (C# `ReadByte` vs `ReadUInt8` naming differences).
    #[inline]
    pub fn read_u8(&mut self) -> IoResult<u8> {
        self.read_byte()
    }

    /// Alias for `read_int16`.
    #[inline]
    pub fn read_i16(&mut self) -> IoResult<i16> {
        self.read_int16()
    }

    /// Alias for `read_uint16` to mirror the C# API more closely.
    #[inline]
    pub fn read_u16(&mut self) -> IoResult<u16> {
        self.read_uint16()
    }

    /// Alias for `read_int32`.
    #[inline]
    pub fn read_i32(&mut self) -> IoResult<i32> {
        self.read_int32()
    }

    /// Alias for `read_uint32` to mirror the C# API more closely.
    #[inline]
    pub fn read_u32(&mut self) -> IoResult<u32> {
        self.read_uint32()
    }

    /// Alias for `read_int64`.
    #[inline]
    pub fn read_i64(&mut self) -> IoResult<i64> {
        self.read_int64()
    }

    /// Alias for `read_uint64` to mirror the C# API more closely.
    #[inline]
    pub fn read_u64(&mut self) -> IoResult<u64> {
        self.read_uint64()
    }

    /// Reads a variable-length integer (C# `ReadVarInt`)
    #[inline]
    pub fn read_var_int(&mut self, max: u64) -> IoResult<u64> {
        let unread = &self.buffer[self.position..];
        let (value, width) = match read_var_int_prefix(unread) {
            Some(decoded) => decoded,
            None => {
                if !unread.is_empty() {
                    self.position += 1;
                }
                return Err(IoError::Format);
            }
        };
        self.position += width;
        if value > max {
            return Err(IoError::Format);
        }
        Ok(value)
    }

    /// Reads a variable-length integer with no upper bound (alias for C# `ReadVarInt` without `max`).
    #[inline]
    pub fn read_var_uint(&mut self) -> IoResult<u64> {
        self.read_var_int(u64::MAX)
    }

    /// Reads a fixed-length string (C# `ReadFixedString`)
    #[inline]
    pub fn read_fixed_string(&mut self, length: usize) -> IoResult<String> {
        let slice = self.read_slice(length)?;
        let mut end = 0;
        while end < slice.len() && slice[end] != 0 {
            end += 1;
        }
        for &byte in &slice[end..] {
            if byte != 0 {
                return Err(IoError::Format);
            }
        }
        let text = str::from_utf8(&slice[..end]).map_err(|_| IoError::InvalidUtf8)?;
        Ok(text.to_string())
    }

    /// Reads a variable-length string (C# `ReadVarString`)
    #[inline]
    pub fn read_var_string(&mut self, max: usize) -> IoResult<String> {
        let length = self.read_var_int(max as u64)? as usize;
        let data = self.read_slice(length)?;
        let text = str::from_utf8(data).map_err(|_| IoError::InvalidUtf8)?;
        Ok(text.to_string())
    }

    /// Reads memory (C# `ReadMemory`)
    #[inline]
    pub fn read_memory(&mut self, count: usize) -> IoResult<&'a [u8]> {
        self.read_slice(count)
    }

    /// Reads variable-length memory (C# `ReadVarMemory`)
    #[inline]
    pub fn read_var_memory(&mut self, max: usize) -> IoResult<&'a [u8]> {
        let count = self.read_var_int(max as u64)? as usize;
        self.read_slice(count)
    }

    /// Reads variable-length memory and returns an owned buffer (C# `ReadVarBytes`)
    #[inline]
    pub fn read_var_bytes(&mut self, max: usize) -> IoResult<Vec<u8>> {
        Ok(self.read_var_memory(max)?.to_vec())
    }

    /// Reads to end (C# `ReadToEnd`)
    #[inline]
    pub fn read_to_end(&mut self) -> IoResult<&'a [u8]> {
        let remaining = self.buffer.len().saturating_sub(self.position);
        self.read_slice(remaining)
    }
}

impl From<str::Utf8Error> for IoError {
    fn from(_: str::Utf8Error) -> Self {
        Self::InvalidUtf8
    }
}

impl IoError {
    /// Helper for creating `InvalidData` errors with a default context.
    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData {
            context: "invalid data".to_string(),
            value: message.into(),
        }
    }

    /// Helper for creating `InvalidData` errors with explicit context/value pairs.
    pub fn invalid_data_with_context(context: impl Into<String>, value: impl Into<String>) -> Self {
        Self::InvalidData {
            context: context.into(),
            value: value.into(),
        }
    }
}
