use crate::{BinaryWriter, IoError, IoResult, Serializable, serializable::helper};

/// Extension helpers for [`BinaryWriter`] mirroring
/// `Neo.Extensions.IO.BinaryWriterExtensions`.
pub trait BinaryWriterExtensions {
    /// Writes a serializable value.
    fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()>;

    /// Writes a variable-length collection of serializable values.
    fn write_serializable_collection<T: Serializable>(&mut self, value: &[T]) -> IoResult<()>;

    /// Writes a variable-length collection of nullable serializable values.
    fn write_nullable_array<T: Serializable>(&mut self, value: &[Option<T>]) -> IoResult<()>;

    /// Writes a fixed-width UTF-8 string padded with zero bytes.
    fn write_fixed_string(&mut self, value: &str, length: usize) -> IoResult<()>;

    /// Writes a variable-length UTF-8 string.
    fn write_var_string(&mut self, value: &str) -> IoResult<()>;
}

impl BinaryWriterExtensions for BinaryWriter {
    fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()> {
        value.serialize(self)
    }

    fn write_serializable_collection<T: Serializable>(&mut self, value: &[T]) -> IoResult<()> {
        helper::serialize_array(value, self)
    }

    fn write_nullable_array<T: Serializable>(&mut self, value: &[Option<T>]) -> IoResult<()> {
        self.write_var_int(value.len() as u64)?;
        for element in value {
            match element {
                Some(item) => {
                    self.write_bool(true)?;
                    item.serialize(self)?;
                }
                None => {
                    self.write_bool(false)?;
                }
            }
        }
        Ok(())
    }

    fn write_fixed_string(&mut self, value: &str, length: usize) -> IoResult<()> {
        if value.len() > length {
            return Err(IoError::invalid_data(format!(
                "String length {} exceeds fixed size {}",
                value.len(),
                length
            )));
        }

        let bytes = value.as_bytes();
        if bytes.len() > length {
            return Err(IoError::invalid_data(format!(
                "UTF-8 byte length {} exceeds fixed size {}",
                bytes.len(),
                length
            )));
        }

        self.write_bytes(bytes)?;
        if bytes.len() < length {
            self.write_bytes(&vec![0u8; length - bytes.len()])?;
        }
        Ok(())
    }

    fn write_var_string(&mut self, value: &str) -> IoResult<()> {
        self.write_var_bytes(value.as_bytes())
    }
}
