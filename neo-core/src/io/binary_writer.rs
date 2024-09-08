use std::io::{self, Write};

/// A binary writer that can write various data types to a byte array.
///
/// # Examples
///
/// ```rust
/// use neo_core::io::binary_writer::BinaryWriter;
/// let mut buffer = Vec::new();
/// let mut writer = BinaryWriter::new(&mut buffer);
///
/// writer.write_u32(42);
/// writer.write_string("Hello, Neo!");
///
/// assert_eq!(buffer, [42, 0, 0, 0, 11, 72, 101, 108, 108, 111, 44, 32, 78, 101, 111, 33]);
/// ```
pub struct BinaryWriter<'a> {
    inner: &'a mut Vec<u8>,
    encoding: &'static encoding_rs::Encoding,
}

impl<'a> BinaryWriter<'a> {
    pub fn new(inner: &'a mut Vec<u8>) -> Self {
        Self::with_encoding(inner, encoding_rs::UTF_8)
    }

    pub fn with_encoding(inner: &'a mut Vec<u8>, encoding: &'static encoding_rs::Encoding) -> Self {
        Self {
            inner,
            encoding,
        }
    }

    pub fn into_inner(self) -> &'a mut Vec<u8> {
        self.inner
    }

    pub fn get_ref(&self) -> &Vec<u8> {
        self.inner
    }

    pub fn get_mut(&mut self) -> &mut Vec<u8> {
        self.inner
    }

    pub fn write_bool(&mut self, value: bool) {
        self.inner.push(value as u8);
    }

    pub fn write_u8(&mut self, value: u8) {
        self.inner.push(value);
    }

    pub fn write_i8(&mut self, value: i8) {
        self.inner.push(value as u8);
    }

    pub fn write_u16(&mut self, value: u16) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_i16(&mut self, value: i16) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_i32(&mut self, value: i32) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_i64(&mut self, value: i64) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_f32(&mut self, value: f32) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_f64(&mut self, value: f64) {
        self.inner.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, buffer: &[u8]) {
        self.inner.extend_from_slice(buffer);
    }

    pub fn write_string(&mut self, value: &str) {
        let (cow, _, _) = self.encoding.encode(value);
        let bytes = cow.as_ref();
        self.write_7bit_encoded_int(bytes.len() as i32);
        self.inner.extend_from_slice(bytes);
    }

    pub fn write_7bit_encoded_int(&mut self, mut value: i32) {
        while value > 0x7F {
            self.inner.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        self.inner.push(value as u8);
    }

    pub fn write_7bit_encoded_int64(&mut self, mut value: i64) {
        while value > 0x7F {
            self.inner.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        self.inner.push(value as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_writer() {
        let mut buffer = Vec::new();
        let mut writer = BinaryWriter::new(&mut buffer);

        // Write various data types
        writer.write_bool(true);
        writer.write_u8(42);
        writer.write_i16(-1000);
        writer.write_u32(1_000_000);
        writer.write_i64(-1_000_000_000_000);
        writer.write_f32(3.14159);
        writer.write_string("Hello, Neo!");
        writer.write_7bit_encoded_int(1000);
        writer.write_7bit_encoded_int64(1_000_000_000_000);

        // Verify the written data
        let mut expected = Vec::new();
        expected.extend_from_slice(&[1]); // bool
        expected.extend_from_slice(&[42]); // u8
        expected.extend_from_slice(&(-1000i16).to_le_bytes()); // i16
        expected.extend_from_slice(&1_000_000u32.to_le_bytes()); // u32
        expected.extend_from_slice(&(-1_000_000_000_000i64).to_le_bytes()); // i64
        expected.extend_from_slice(&3.14159f32.to_le_bytes()); // f32
        expected.extend_from_slice(&[11]); // string length
        expected.extend_from_slice(b"Hello, Neo!"); // string content
        expected.extend_from_slice(&[0xE8, 0x07]); // 7-bit encoded int (1000)
        expected.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xE8, 0x3E]); // 7-bit encoded int64 (1_000_000_000_000)

        assert_eq!(buffer, expected);
    }
}
