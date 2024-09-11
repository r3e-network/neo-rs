use std::convert::TryInto;
use std::mem::size_of;
use byteorder::{ByteOrder, LittleEndian, BigEndian};

pub struct MemoryReader<'a> {
    memory: &'a [u8],
    pos: usize,
}

impl<'a> MemoryReader<'a> {
    pub fn new(memory: &'a [u8]) -> Self {
        Self { memory, pos: 0 }
    }

    #[inline(always)]
    fn ensure_position(&self, move_by: usize) -> Result<(), std::io::Error> {
        if self.pos + move_by > self.memory.len() {
            Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected end of data"))
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    pub fn position(&self) -> usize {
        self.pos
    }

    #[inline(always)]
    pub fn peek(&self) -> Result<u8, std::io::Error> {
        self.ensure_position(1)?;
        Ok(self.memory[self.pos])
    }

    pub fn read_bool(&mut self) -> Result<bool, std::io::Error> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid boolean value")),
        }
    }

    #[inline(always)]
    pub fn read_i8(&mut self) -> Result<i8, std::io::Error> {
        self.ensure_position(1)?;
        let value = self.memory[self.pos] as i8;
        self.pos += 1;
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u8(&mut self) -> Result<u8, std::io::Error> {
        self.ensure_position(1)?;
        let value = self.memory[self.pos];
        self.pos += 1;
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i16(&mut self) -> Result<i16, std::io::Error> {
        self.ensure_position(size_of::<i16>())?;
        let value = LittleEndian::read_i16(&self.memory[self.pos..]);
        self.pos += size_of::<i16>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i16_big_endian(&mut self) -> Result<i16, std::io::Error> {
        self.ensure_position(size_of::<i16>())?;
        let value = BigEndian::read_i16(&self.memory[self.pos..]);
        self.pos += size_of::<i16>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u16(&mut self) -> Result<u16, std::io::Error> {
        self.ensure_position(size_of::<u16>())?;
        let value = LittleEndian::read_u16(&self.memory[self.pos..]);
        self.pos += size_of::<u16>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u16_big_endian(&mut self) -> Result<u16, std::io::Error> {
        self.ensure_position(size_of::<u16>())?;
        let value = BigEndian::read_u16(&self.memory[self.pos..]);
        self.pos += size_of::<u16>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i32(&mut self) -> Result<i32, std::io::Error> {
        self.ensure_position(size_of::<i32>())?;
        let value = LittleEndian::read_i32(&self.memory[self.pos..]);
        self.pos += size_of::<i32>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i32_big_endian(&mut self) -> Result<i32, std::io::Error> {
        self.ensure_position(size_of::<i32>())?;
        let value = BigEndian::read_i32(&self.memory[self.pos..]);
        self.pos += size_of::<i32>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u32(&mut self) -> Result<u32, std::io::Error> {
        self.ensure_position(size_of::<u32>())?;
        let value = LittleEndian::read_u32(&self.memory[self.pos..]);
        self.pos += size_of::<u32>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u32_big_endian(&mut self) -> Result<u32, std::io::Error> {
        self.ensure_position(size_of::<u32>())?;
        let value = BigEndian::read_u32(&self.memory[self.pos..]);
        self.pos += size_of::<u32>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i64(&mut self) -> Result<i64, std::io::Error> {
        self.ensure_position(size_of::<i64>())?;
        let value = LittleEndian::read_i64(&self.memory[self.pos..]);
        self.pos += size_of::<i64>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_i64_big_endian(&mut self) -> Result<i64, std::io::Error> {
        self.ensure_position(size_of::<i64>())?;
        let value = BigEndian::read_i64(&self.memory[self.pos..]);
        self.pos += size_of::<i64>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u64(&mut self) -> Result<u64, std::io::Error> {
        self.ensure_position(size_of::<u64>())?;
        let value = LittleEndian::read_u64(&self.memory[self.pos..]);
        self.pos += size_of::<u64>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u64_big_endian(&mut self) -> Result<u64, std::io::Error> {
        self.ensure_position(size_of::<u64>())?;
        let value = BigEndian::read_u64(&self.memory[self.pos..]);
        self.pos += size_of::<u64>();
        Ok(value)
    }

    #[inline(always)]
    pub fn read_var_int(&mut self, max: u64) -> Result<u64, std::io::Error> {
        let b = self.read_u8()?;
        let value = match b {
            0xfd => self.read_u16()? as u64,
            0xfe => self.read_u32()? as u64,
            0xff => self.read_u64()?,
            _ => b as u64,
        };
        if value > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt exceeds maximum value"));
        }
        Ok(value)
    }

    #[inline(always)]
    pub fn read_fixed_string(&mut self, length: usize) -> Result<String, std::io::Error> {
        self.ensure_position(length)?;
        let end = self.pos + length;
        let mut i = self.pos;
        while i < end && self.memory[i] != 0 {
            i += 1;
        }
        let data = &self.memory[self.pos..i];
        for j in i..end {
            if self.memory[j] != 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid fixed string format"));
            }
        }
        self.pos = end;
        String::from_utf8(data.to_vec()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 sequence"))
    }

    #[inline(always)]
    pub fn read_var_string(&mut self, max: usize) -> Result<String, std::io::Error> {
        let length = self.read_var_int(max as u64)? as usize;
        self.ensure_position(length)?;
        let data = &self.memory[self.pos..self.pos + length];
        self.pos += length;
        String::from_utf8(data.to_vec()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 sequence"))
    }

    #[inline(always)]
    pub fn read_memory(&mut self, count: usize) -> Result<&'a [u8], std::io::Error> {
        self.ensure_position(count)?;
        let result = &self.memory[self.pos..self.pos + count];
        self.pos += count;
        Ok(result)
    }

    #[inline(always)]
    pub fn read_var_memory(&mut self, max: usize) -> Result<&'a [u8], std::io::Error> {
        let length = self.read_var_int(max as u64)? as usize;
        self.read_memory(length)
    }

    #[inline(always)]
    pub fn read_to_end(&mut self) -> &'a [u8] {
        let result = &self.memory[self.pos..];
        self.pos = self.memory.len();
        result
    }
}
