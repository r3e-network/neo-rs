use crate::io::{BinReader, BinWriter};

// MaxSize is the maximum payload size in decompressed form.
const MAX_SIZE: usize = 0x02000000;

// Payload is anything that can be binary encoded/decoded.
pub trait Payload: BinSerializable {}

// NullPayload is a dummy payload with no fields.
pub struct NullPayload;

// NewNullPayload returns zero-sized stub payload.
impl NullPayload {
    pub fn new() -> Self {
        NullPayload
    }
}

// Implementing the BinSerializable trait for NullPayload
impl BinSerializable for NullPayload {
    fn decode_binary(&mut self, _r: &mut BinReader) {}

    fn encode_binary(&self, _w: &mut BinWriter) {}
}
