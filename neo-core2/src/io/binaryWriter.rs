use std::io::{self, Write};
use std::any::Any;
use std::mem::size_of;
use std::slice;

pub struct BinWriter<W: Write> {
    w: W,
    err: Option<io::Error>,
    uv: [u8; 9],
}

impl<W: Write> BinWriter<W> {
    pub fn new(iow: W) -> Self {
        BinWriter {
            w: iow,
            err: None,
            uv: [0; 9],
        }
    }

    pub fn write_u64_le(&mut self, u64: u64) {
        if self.err.is_some() {
            return;
        }
        self.uv[..8].copy_from_slice(&u64.to_le_bytes());
        self.write_bytes(&self.uv[..8]);
    }

    pub fn write_u32_le(&mut self, u32: u32) {
        if self.err.is_some() {
            return;
        }
        self.uv[..4].copy_from_slice(&u32.to_le_bytes());
        self.write_bytes(&self.uv[..4]);
    }

    pub fn write_u16_le(&mut self, u16: u16) {
        if self.err.is_some() {
            return;
        }
        self.uv[..2].copy_from_slice(&u16.to_le_bytes());
        self.write_bytes(&self.uv[..2]);
    }

    pub fn write_u16_be(&mut self, u16: u16) {
        if self.err.is_some() {
            return;
        }
        self.uv[..2].copy_from_slice(&u16.to_be_bytes());
        self.write_bytes(&self.uv[..2]);
    }

    pub fn write_b(&mut self, u8: u8) {
        if self.err.is_some() {
            return;
        }
        self.uv[0] = u8;
        self.write_bytes(&self.uv[..1]);
    }

    pub fn write_bool(&mut self, b: bool) {
        if self.err.is_some() {
            return;
        }
        self.write_b(if b { 1 } else { 0 });
    }

    pub fn write_array<T: Encodable>(&mut self, arr: &[T]) {
        if self.err.is_some() {
            return;
        }
        self.write_var_uint(arr.len() as u64);
        for el in arr {
            el.encode_binary(self);
        }
    }

    pub fn write_var_uint(&mut self, val: u64) {
        if self.err.is_some() {
            return;
        }
        let n = put_var_uint(&mut self.uv, val);
        self.write_bytes(&self.uv[..n]);
    }

    pub fn write_bytes(&mut self, b: &[u8]) {
        if self.err.is_some() {
            return;
        }
        if let Err(e) = self.w.write_all(b) {
            self.err = Some(e);
        }
    }

    pub fn write_var_bytes(&mut self, b: &[u8]) {
        self.write_var_uint(b.len() as u64);
        self.write_bytes(b);
    }

    pub fn write_string(&mut self, s: &str) {
        self.write_var_uint(s.len() as u64);
        if self.err.is_some() {
            return;
        }
        if let Err(e) = self.w.write_all(s.as_bytes()) {
            self.err = Some(e);
        }
    }

    pub fn grow(&mut self, n: usize) {
        if let Some(b) = self.w.by_ref().downcast_mut::<Vec<u8>>() {
            b.reserve(n);
        }
    }
}

pub fn put_var_uint(data: &mut [u8], val: u64) -> usize {
    if val < 0xfd {
        data[0] = val as u8;
        1
    } else if val <= 0xffff {
        data[0] = 0xfd;
        data[1..3].copy_from_slice(&(val as u16).to_le_bytes());
        3
    } else if val <= 0xffff_ffff {
        data[0] = 0xfe;
        data[1..5].copy_from_slice(&(val as u32).to_le_bytes());
        5
    } else {
        data[0] = 0xff;
        data[1..9].copy_from_slice(&val.to_le_bytes());
        9
    }
}

pub trait Encodable {
    fn encode_binary(&self, writer: &mut BinWriter<impl Write>);
}
