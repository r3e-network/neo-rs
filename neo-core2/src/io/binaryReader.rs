use std::io::{self, Read};
use std::fmt;
use std::slice;
use std::any::Any;
use std::mem;

const MAX_ARRAY_SIZE: usize = 0x1000000;

pub struct BinReader<R: Read> {
    r: R,
    uv: [u8; 8],
    err: Option<io::Error>,
}

impl<R: Read> BinReader<R> {
    pub fn new(r: R) -> Self {
        BinReader {
            r,
            uv: [0; 8],
            err: None,
        }
    }

    pub fn from_buf(buf: &[u8]) -> Self {
        BinReader::new(io::Cursor::new(buf))
    }

    pub fn len(&mut self) -> isize {
        if let Some(cursor) = self.r.by_ref().downcast_ref::<io::Cursor<&[u8]>>() {
            cursor.get_ref().len() as isize - cursor.position() as isize
        } else {
            -1
        }
    }

    pub fn read_u64_le(&mut self) -> u64 {
        self.read_bytes(&mut self.uv[..8]);
        if self.err.is_some() {
            return 0;
        }
        u64::from_le_bytes(self.uv[..8].try_into().unwrap())
    }

    pub fn read_u32_le(&mut self) -> u32 {
        self.read_bytes(&mut self.uv[..4]);
        if self.err.is_some() {
            return 0;
        }
        u32::from_le_bytes(self.uv[..4].try_into().unwrap())
    }

    pub fn read_u16_le(&mut self) -> u16 {
        self.read_bytes(&mut self.uv[..2]);
        if self.err.is_some() {
            return 0;
        }
        u16::from_le_bytes(self.uv[..2].try_into().unwrap())
    }

    pub fn read_u16_be(&mut self) -> u16 {
        self.read_bytes(&mut self.uv[..2]);
        if self.err.is_some() {
            return 0;
        }
        u16::from_be_bytes(self.uv[..2].try_into().unwrap())
    }

    pub fn read_b(&mut self) -> u8 {
        self.read_bytes(&mut self.uv[..1]);
        if self.err.is_some() {
            return 0;
        }
        self.uv[0]
    }

    pub fn read_bool(&mut self) -> bool {
        self.read_b() != 0
    }

    pub fn read_array<T: Decodable>(&mut self, t: &mut Vec<T>, max_size: Option<usize>) {
        if self.err.is_some() {
            return;
        }

        let ms = max_size.unwrap_or(MAX_ARRAY_SIZE);
        let lu = self.read_var_uint();
        if lu > ms as u64 {
            self.err = Some(io::Error::new(io::ErrorKind::InvalidData, format!("array is too big ({})", lu)));
            return;
        }

        let l = lu as usize;
        t.reserve(l);

        for _ in 0..l {
            let mut elem = T::default();
            elem.decode_binary(self);
            t.push(elem);
        }
    }

    pub fn read_var_uint(&mut self) -> u64 {
        if self.err.is_some() {
            return 0;
        }

        let b = self.read_b();
        match b {
            0xfd => self.read_u16_le() as u64,
            0xfe => self.read_u32_le() as u64,
            0xff => self.read_u64_le(),
            _ => b as u64,
        }
    }

    pub fn read_var_bytes(&mut self, max_size: Option<usize>) -> Vec<u8> {
        let n = self.read_var_uint();
        let ms = max_size.unwrap_or(MAX_ARRAY_SIZE);
        if n > ms as u64 {
            self.err = Some(io::Error::new(io::ErrorKind::InvalidData, format!("byte-slice is too big ({})", n)));
            return Vec::new();
        }
        let mut b = vec![0; n as usize];
        self.read_bytes(&mut b);
        b
    }

    pub fn read_bytes(&mut self, buf: &mut [u8]) {
        if self.err.is_some() {
            return;
        }

        if let Err(e) = self.r.read_exact(buf) {
            self.err = Some(e);
        }
    }

    pub fn read_string(&mut self, max_size: Option<usize>) -> String {
        let b = self.read_var_bytes(max_size);
        String::from_utf8(b).unwrap_or_default()
    }
}

pub trait Decodable: Default {
    fn decode_binary(&mut self, reader: &mut BinReader<impl Read>);
}
