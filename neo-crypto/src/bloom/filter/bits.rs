use core::cmp::min;

pub(crate) fn set_bit(bits: &mut [u8], index: usize) {
    let byte = index / 8;
    let offset = (index % 8) as u8;
    if let Some(slot) = bits.get_mut(byte) {
        *slot |= 1 << offset;
    }
}

pub(crate) fn test_bit(bits: &[u8], index: usize) -> bool {
    let byte = index / 8;
    let offset = (index % 8) as u8;
    bits.get(byte)
        .map(|slot| (slot >> offset) & 1 == 1)
        .unwrap_or(false)
}

pub(crate) fn copy_bits(src: &[u8], out: &mut [u8]) {
    let copy_len = min(out.len(), src.len());
    out[..copy_len].copy_from_slice(&src[..copy_len]);
    if out.len() > copy_len {
        for byte in &mut out[copy_len..] {
            *byte = 0;
        }
    }
}
