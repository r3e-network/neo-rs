use std::error::Error;
use std::convert::TryInto;
use byteorder::{ByteOrder, LittleEndian};
use lz4::block::{compress, decompress};
use crate::network::payload;

// compress compresses bytes using lz4.
pub fn compress(source: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut dest = vec![0u8; 4 + lz4::block::compress_bound(source.len())?];
    let size = compress(source, &mut dest[4..], None)?;
    LittleEndian::write_u32(&mut dest[..4], source.len() as u32);
    dest.truncate(size + 4);
    Ok(dest)
}

// decompress decompresses bytes using lz4.
pub fn decompress(source: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    if source.len() < 4 {
        return Err("invalid compressed payload".into());
    }
    let length = LittleEndian::read_u32(&source[..4]) as usize;
    if length > payload::MAX_SIZE {
        return Err("invalid uncompressed payload length".into());
    }
    let mut dest = vec![0u8; length];
    let size = decompress(&source[4..], &mut dest)?;
    if size != length {
        return Err("decompressed payload size doesn't match header".into());
    }
    Ok(dest)
}
