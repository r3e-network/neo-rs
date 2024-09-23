use std::io::{self, Write};
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct ErrDrained;

impl fmt::Display for ErrDrained {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "buffer already drained")
    }
}

impl Error for ErrDrained {}

pub struct BufBinWriter {
    bin_writer: BinWriter,
    buf: Vec<u8>,
    err: Option<Box<dyn Error>>,
}

impl BufBinWriter {
    // NewBufBinWriter makes a BufBinWriter with an empty byte buffer.
    pub fn new() -> BufBinWriter {
        let buf = Vec::new();
        let bin_writer = BinWriter::new(&buf);
        BufBinWriter {
            bin_writer,
            buf,
            err: None,
        }
    }

    // Len returns the number of bytes of the unread portion of the buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    // Bytes returns the resulting buffer and makes future writes return an error.
    pub fn bytes(&mut self) -> Option<&[u8]> {
        if self.err.is_some() {
            return None;
        }
        self.err = Some(Box::new(ErrDrained));
        Some(&self.buf)
    }

    // Reset resets the state of the buffer, making it usable again. It can
    // make buffer usage somewhat more efficient because you don't need to
    // create it again. But beware, the buffer is gonna be the same as the one
    // returned by Bytes(), so if you need that data after Reset() you have to copy
    // it yourself.
    pub fn reset(&mut self) {
        self.err = None;
        self.buf.clear();
    }
}
