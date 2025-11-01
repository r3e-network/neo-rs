use crate::{serializable::Serializable, IoResult};

#[derive(Debug, Clone, Default)]
pub struct BinaryWriter {
    buffer: Vec<u8>,
}

impl BinaryWriter {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn write_u8(&mut self, value: u8) -> IoResult<()> {
        self.buffer.push(value);
        Ok(())
    }

    #[inline]
    pub fn write_byte(&mut self, value: u8) -> IoResult<()> {
        self.write_u8(value)
    }

    pub fn write_bool(&mut self, value: bool) -> IoResult<()> {
        self.write_u8(if value { 1 } else { 0 })
    }

    pub fn write_u16(&mut self, value: u16) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_i16(&mut self, value: i16) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_u32(&mut self, value: u32) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_i32(&mut self, value: i32) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_i64(&mut self, value: i64) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_u64(&mut self, value: u64) -> IoResult<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> IoResult<()> {
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn write_var_int(&mut self, value: u64) -> IoResult<()> {
        if value < 0xFD {
            self.write_u8(value as u8)?;
        } else if value <= 0xFFFF {
            self.write_u8(0xFD)?;
            self.write_u16(value as u16)?;
        } else if value <= 0xFFFF_FFFF {
            self.write_u8(0xFE)?;
            self.write_u32(value as u32)?;
        } else {
            self.write_u8(0xFF)?;
            self.write_u64(value)?;
        }
        Ok(())
    }

    #[inline]
    pub fn write_var_uint(&mut self, value: u64) -> IoResult<()> {
        self.write_var_int(value)
    }

    pub fn write_var_bytes(&mut self, bytes: &[u8]) -> IoResult<()> {
        self.write_var_int(bytes.len() as u64)?;
        self.write_bytes(bytes)
    }

    pub fn write_var_string(&mut self, value: &str) -> IoResult<()> {
        self.write_var_bytes(value.as_bytes())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.buffer.clone()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    pub fn write_all(&mut self, data: &[u8]) -> IoResult<()> {
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    pub fn write_serializable<T: Serializable>(&mut self, value: &T) -> IoResult<()> {
        value.serialize(self)
    }

    pub fn write_serializable_vec<T: Serializable>(&mut self, values: &[T]) -> IoResult<()> {
        self.write_var_uint(values.len() as u64)?;
        for value in values {
            value.serialize(self)?;
        }
        Ok(())
    }
}
