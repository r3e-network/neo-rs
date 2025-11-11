use neo_base::hash::murmur32;

pub(crate) fn hash_element(element: &[u8], seed: u32, bit_len: u32) -> usize {
    let hash = murmur32(element, seed);
    (hash % bit_len) as usize
}
