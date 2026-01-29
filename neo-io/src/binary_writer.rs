use crate::{serializable::Serializable, IoResult};

#[derive(Debug, Clone, Default)]
pub struct BinaryWriter {
    buffer: Vec<u8>,
}

impl BinaryWriter {
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

    #[must_use] 
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true when the writer contains zero bytes.
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
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
        self.write_u8(u8::from(value))
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

    /// Returns a reference to the internal buffer without cloning (zero-copy).
    /// 
    /// Use this when you only need to read the serialized bytes without taking ownership.
    /// For an owned copy, use `to_bytes()`; to take ownership, use `into_bytes()`.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    #[must_use] 
    pub fn to_bytes(&self) -> Vec<u8> {
        self.buffer.clone()
    }

    #[must_use] 
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
