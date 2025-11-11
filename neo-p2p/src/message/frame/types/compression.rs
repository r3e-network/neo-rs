use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use neo_base::encoding::DecodeError;

use crate::message::frame::PAYLOAD_MAX_SIZE;

pub(super) fn try_compress(data: &[u8]) -> Result<Vec<u8>, ()> {
    Ok(compress_prepend_size(data))
}

pub(super) fn decompress_payload(data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if data.len() < 4 {
        return Err(DecodeError::InvalidValue("compressed payload too short"));
    }
    let declared = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if declared > PAYLOAD_MAX_SIZE {
        return Err(DecodeError::LengthOutOfRange {
            len: declared as u64,
            max: PAYLOAD_MAX_SIZE as u64,
        });
    }
    decompress_size_prepended(data).map_err(|_| DecodeError::InvalidValue("lz4 payload"))
}
