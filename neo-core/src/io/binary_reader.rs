use std::io::{self, Read};

/// A binary reader that can read various data types from a byte array.
///
/// # Examples
///
/// ```rust
/// use neo_core::io::binary_reader::BinaryReader;
/// let buffer = vec![42, 0, 0, 0, 11, 72, 101, 108, 108, 111, 44, 32, 78, 101, 111, 33];
/// let mut reader = BinaryReader::new(&buffer[..]);
///
/// assert_eq!(reader.read_u32().unwrap(), 42);
/// assert_eq!(reader.read_string().unwrap(), "Hello, Neo!");
/// ```
pub struct BinaryReader<'a> {
    inner: &'a [u8],
    position: usize,
    encoding: &'static encoding_rs::Encoding,
}

impl<'a> BinaryReader<'a> {
    pub fn new(inner: &'a [u8]) -> Self {
        Self::with_encoding(inner, encoding_rs::UTF_8)
    }

    pub fn with_encoding(inner: &'a [u8], encoding: &'static encoding_rs::Encoding) -> Self {
        Self {
            inner,
            position: 0,
            encoding,
        }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn set_position(&mut self, position: usize) {
        self.position = position.min(self.inner.len());
    }

    pub fn read_bool(&mut self) -> io::Result<bool> {
        self.read_u8().map(|v| v != 0)
    }

    pub fn read_u8(&mut self) -> io::Result<u8> {
        if self.position < self.inner.len() {
            let value = self.inner[self.position];
            self.position += 1;
            Ok(value)
        } else {
            Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"))
        }
    }

    pub fn read_i8(&mut self) -> io::Result<i8> {
        self.read_u8().map(|v| v as i8)
    }

    pub fn read_u16(&mut self) -> io::Result<u16> {
        let mut buffer = [0u8; 2];
        self.read_exact(&mut buffer)?;
        Ok(u16::from_le_bytes(buffer))
    }

    pub fn read_i16(&mut self) -> io::Result<i16> {
        self.read_u16().map(|v| v as i16)
    }

    pub fn read_u32(&mut self) -> io::Result<u32> {
        let mut buffer = [0u8; 4];
        self.read_exact(&mut buffer)?;
        Ok(u32::from_le_bytes(buffer))
    }

    pub fn read_i32(&mut self) -> io::Result<i32> {
        self.read_u32().map(|v| v as i32)
    }

    pub fn read_u64(&mut self) -> io::Result<u64> {
        let mut buffer = [0u8; 8];
        self.read_exact(&mut buffer)?;
        Ok(u64::from_le_bytes(buffer))
    }

    pub fn read_i64(&mut self) -> io::Result<i64> {
        self.read_u64().map(|v| v as i64)
    }

    pub fn read_f32(&mut self) -> io::Result<f32> {
        let mut buffer = [0u8; 4];
        self.read_exact(&mut buffer)?;
        Ok(f32::from_le_bytes(buffer))
    }

    pub fn read_f64(&mut self) -> io::Result<f64> {
        let mut buffer = [0u8; 8];
        self.read_exact(&mut buffer)?;
        Ok(f64::from_le_bytes(buffer))
    }

    pub fn read_bytes(&mut self, count: usize) -> io::Result<Vec<u8>> {
        let mut buffer = vec![0u8; count];
        self.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    pub fn read_string(&mut self) -> io::Result<String> {
        let length = self.read_7bit_encoded_int()? as usize;
        let bytes = self.read_bytes(length)?;
        let (cow, _, had_errors) = self.encoding.decode(&bytes);
        if had_errors {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid string encoding"));
        }
        Ok(cow.into_owned())
    }

    pub fn read_7bit_encoded_int(&mut self) -> io::Result<i32> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as i32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(result)
    }

    pub fn read_7bit_encoded_int64(&mut self) -> io::Result<i64> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as i64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(result)
    }
}

impl<'a> Read for BinaryReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.inner.len() - self.position;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&self.inner[self.position..self.position + to_read]);
        self.position += to_read;
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_reader() {
        let buffer = vec![
            1, // bool
            42, // u8
            0xE8, 0xFC, // i16 (-1000)
            0x40, 0x42, 0x0F, 0x00, // u32 (1_000_000)
            0x00, 0x00, 0x00, 0x00, 0x00, 0xE8, 0x76, 0xF3, // i64 (-1_000_000_000_000)
            0xDB, 0x0F, 0x49, 0x40, // f32 (3.14159)
            11, // string length
            72, 101, 108, 108, 111, 44, 32, 78, 101, 111, 33, // "Hello, Neo!"
            0xE8, 0x07, // 7-bit encoded int (1000)
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xE8, 0x3E, // 7-bit encoded int64 (1_000_000_000_000)
        ];

        let mut reader = BinaryReader::new(&buffer);

        assert_eq!(reader.read_bool().unwrap(), true);
        assert_eq!(reader.read_u8().unwrap(), 42);
        assert_eq!(reader.read_i16().unwrap(), -1000);
        assert_eq!(reader.read_u32().unwrap(), 1_000_000);
        assert_eq!(reader.read_i64().unwrap(), -1_000_000_000_000);
        assert!((reader.read_f32().unwrap() - 3.14159).abs() < f32::EPSILON);
        assert_eq!(reader.read_string().unwrap(), "Hello, Neo!");
        assert_eq!(reader.read_7bit_encoded_int().unwrap(), 1000);
        assert_eq!(reader.read_7bit_encoded_int64().unwrap(), 1_000_000_000_000);
    }
}
