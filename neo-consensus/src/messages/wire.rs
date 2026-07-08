//! Shared dBFT message body wire helpers.

use neo_io::var_int::VarInt;
use neo_primitives::UInt256;

pub(super) fn append_uint256_array(dst: &mut Vec<u8>, hashes: &[UInt256]) {
    VarInt::write_var_int(hashes.len() as u64, dst);
    for hash in hashes {
        dst.extend_from_slice(&hash.as_bytes());
    }
}

pub(super) fn uint256_array_encoded_len(hashes: &[UInt256]) -> usize {
    VarInt::encoded_len(hashes.len() as u64) + hashes.len() * UInt256::LENGTH
}
