use std::io::{Error, ErrorKind, Read, Result};

/// Extension helpers for [`Read`] mirroring `Neo.Extensions.IO.BinaryReaderExtensions`.
pub trait BinaryReaderExtensions: Read {
    /// Reads exactly `size` bytes from the underlying stream.
    fn read_fixed_bytes(&mut self, size: usize) -> Result<Vec<u8>>;

    /// Reads a variable-length byte array (Neo var-bytes encoding).
    fn read_var_bytes(&mut self, max: usize) -> Result<Vec<u8>>;

    /// Reads a variable-length integer (Neo var-int encoding).
    fn read_var_int(&mut self, max: u64) -> Result<u64>;
}

impl<T: Read> BinaryReaderExtensions for T {
    fn read_fixed_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut data = vec![0u8; size];
        let mut offset = 0;
        while offset < size {
            let read = self.read(&mut data[offset..])?;
            if read == 0 {
                return Err(Error::new(
                    ErrorKind::UnexpectedEof,
                    "Unexpected end of stream",
                ));
            }
            offset += read;
        }
        Ok(data)
    }

    fn read_var_bytes(&mut self, max: usize) -> Result<Vec<u8>> {
        let length = self.read_var_int(max as u64)? as usize;
        if length > max {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("Var-bytes length {length} exceeds maximum {max}"),
            ));
        }
        self.read_fixed_bytes(length)
    }

    fn read_var_int(&mut self, max: u64) -> Result<u64> {
        let mut prefix = [0u8; 1];
        self.read_exact(&mut prefix)?;
        let value = match prefix[0] {
            0xFD => {
                let mut buf = [0u8; 2];
                self.read_exact(&mut buf)?;
                u16::from_le_bytes(buf) as u64
            }
            0xFE => {
                let mut buf = [0u8; 4];
                self.read_exact(&mut buf)?;
                u32::from_le_bytes(buf) as u64
            }
            0xFF => {
                let mut buf = [0u8; 8];
                self.read_exact(&mut buf)?;
                u64::from_le_bytes(buf)
            }
            value => value as u64,
        };

        if value > max {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("Var-int value {value} exceeds maximum {max}"),
            ));
        }
        Ok(value)
    }
}
