use crate::extensions::byte::ByteExtensions;
use crate::io::{IoError, IoResult, MemoryReader, Serializable};

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
        self.as_serializable_array(max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        if self.is_empty() {
            return Err(IoError::invalid_data(
                "Cannot deserialize from an empty ReadOnlyMemory",
            ));
        }
        let mut reader = MemoryReader::new(self);
        T::deserialize(&mut reader)
    }

    fn get_var_size(&self) -> IoResult<usize> {
        let length = self.len();
        Ok(util::var_size(length) + length)
    }
}

impl ReadOnlyMemoryExtensions for Vec<u8> {
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        self.as_slice().as_serializable_array(max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        self.as_slice().as_serializable()
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
