use alloc::vec::Vec;

use neo_base::hash::double_sha256;

use crate::h256::H256;

pub fn compute_merkle_root(hashes: &[H256]) -> H256 {
    if hashes.is_empty() {
        return H256::default();
    }
    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut layer = hashes.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for chunk in layer.chunks(2) {
            let a = chunk[0];
            let b = if chunk.len() == 2 { chunk[1] } else { chunk[0] };
            let mut buffer = [0u8; 64];
            buffer[..32].copy_from_slice(a.as_ref());
            buffer[32..].copy_from_slice(b.as_ref());
            next.push(H256::from_le_bytes(double_sha256(buffer)));
        }
        layer = next;
    }
    layer[0]
}
