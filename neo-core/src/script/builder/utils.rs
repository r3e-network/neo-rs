use alloc::vec::Vec;

use neo_base::hash::sha256;

pub(crate) fn minimal_signed_bytes(mut value: i64) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    loop {
        result.push((value & 0xFF) as u8);
        value >>= 8;
        let last = *result.last().expect("at least one byte exists");
        let done = (value == 0 && (last & 0x80) == 0) || (value == -1 && (last & 0x80) != 0);
        if done {
            break;
        }
    }
    result
}

pub(crate) fn syscall_hash(name: &str) -> u32 {
    let digest = sha256(name.as_bytes());
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}
