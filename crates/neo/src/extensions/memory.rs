use crate::extensions::byte::ByteExtensions;
use crate::io::{IoError, IoResult, Serializable};

/// Extension helpers for read-only byte slices matching
/// `Neo.Extensions.MemoryExtensions`.
pub trait ReadOnlyMemoryExtensions {
    /// Deserialises the span into a vector of [`Serializable`] values.
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>>;

    /// Deserialises the span into a [`Serializable`] value.
    fn as_serializable<T: Serializable>(&self) -> IoResult<T>;

    /// Gets the size in bytes when encoded using Neo's variable length rules.
    fn get_var_size(&self) -> IoResult<usize>;
}

impl ReadOnlyMemoryExtensions for [u8] {
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        <[u8] as ByteExtensions>::as_serializable_array(self, max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        if self.is_empty() {
            return Err(IoError::invalid_data(
                "Cannot deserialize from an empty ReadOnlyMemory",
            ));
        }
        <[u8] as ByteExtensions>::as_serializable(self, 0)
    }

    fn get_var_size(&self) -> IoResult<usize> {
        let length = self.len();
        Ok(util::var_size(length) + length)
    }
}

impl ReadOnlyMemoryExtensions for Vec<u8> {
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        <[u8] as ReadOnlyMemoryExtensions>::as_serializable_array(self.as_slice(), max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        <[u8] as ReadOnlyMemoryExtensions>::as_serializable(self.as_slice())
    }

    fn get_var_size(&self) -> IoResult<usize> {
        self.as_slice().get_var_size()
    }
}

/// Internal utility helpers.
mod util {
    /// Returns the size required to encode `value` using the Neo var size rules.
    pub fn var_size(value: usize) -> usize {
        if value < 0xFD {
            1
        } else if value <= 0xFFFF {
            3
        } else if value <= 0xFFFF_FFFF {
            5
        } else {
            9
        }
    }
}
