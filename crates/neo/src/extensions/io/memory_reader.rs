use crate::io::{IoError, IoResult, MemoryReader, Serializable};

/// Extension helpers for [`MemoryReader`] mirroring
/// `Neo.Extensions.IO.MemoryReaderExtensions`.
pub trait MemoryReaderExtensions {
    fn read_nullable_array<T: Serializable + Default>(
        &mut self,
        max: usize,
    ) -> IoResult<Vec<Option<T>>>;

    fn read_serializable<T: Serializable>(&mut self) -> IoResult<T>;

    fn read_serializable_array<T: Serializable>(&mut self, max: usize) -> IoResult<Vec<T>>;
}

impl<'a> MemoryReaderExtensions for MemoryReader<'a> {
    fn read_nullable_array<T: Serializable + Default>(
        &mut self,
        max: usize,
    ) -> IoResult<Vec<Option<T>>> {
        let count = self.read_var_uint()? as usize;
        if count > max {
            return Err(IoError::invalid_data(format!(
                "Nullable array length {} exceeds maximum {}",
                count, max
            )));
        }

        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let has_value = self.read_boolean()?;
            if has_value {
                items.push(Some(T::deserialize(self)?));
            } else {
                items.push(None);
            }
        }
        Ok(items)
    }

    fn read_serializable<T: Serializable>(&mut self) -> IoResult<T> {
        T::deserialize(self)
    }

    fn read_serializable_array<T: Serializable>(&mut self, max: usize) -> IoResult<Vec<T>> {
        let count = self.read_var_uint()? as usize;
        if count > max {
            return Err(IoError::invalid_data(format!(
                "Array length {} exceeds maximum {}",
                count, max
            )));
        }

        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(T::deserialize(self)?);
        }
        Ok(items)
    }
}
