use crate::io::{BinaryWriter, IoError, IoResult, Serializable};

/// Extension helpers for [`BinaryWriter`] mirroring
/// `Neo.Extensions.IO.BinaryWriterExtensions`.
pub trait BinaryWriterExtensions {
    fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()>;

    fn write_serializable_collection<T: Serializable>(&mut self, value: &[T]) -> IoResult<()>;

    fn write_nullable_array<T: Serializable>(&mut self, value: &[Option<T>]) -> IoResult<()>;

    fn write_fixed_string(&mut self, value: &str, length: usize) -> IoResult<()>;

    fn write_var_string(&mut self, value: &str) -> IoResult<()>;
}

impl BinaryWriterExtensions for BinaryWriter {
    fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()> {
        value.serialize(self)
    }

    fn write_serializable_collection<T: Serializable>(&mut self, value: &[T]) -> IoResult<()> {
        self.write_var_uint(value.len() as u64)?;
        for item in value {
            item.serialize(self)?;
        }
        Ok(())
    }

    fn write_nullable_array<T: Serializable>(&mut self, value: &[Option<T>]) -> IoResult<()> {
        self.write_var_uint(value.len() as u64)?;
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
