use alloc::vec::Vec;

use crate::hash::{double_sha256, Hash256};

/// Build a Merkle tree using double SHA-256 hashing and return the root hash.
pub fn merkle_root(leaves: &[Hash256]) -> Hash256 {
    match leaves.len() {
        0 => Hash256::ZERO,
        1 => leaves[0],
        _ => {
            let mut level: Vec<Hash256> = leaves.to_vec();
            while level.len() > 1 {
                let mut next = Vec::with_capacity((level.len() + 1) / 2);
                for chunk in level.chunks(2) {
                    let left = chunk[0].as_slice();
                    let right = if chunk.len() == 2 {
                        chunk[1].as_slice()
                    } else {
                        left
                    };
                    let mut buffer = [0u8; 64];
                    buffer[..32].copy_from_slice(left);
                    buffer[32..].copy_from_slice(right);
                    next.push(Hash256::new(double_sha256(&buffer)));
                }
                level = next;
            }
            level[0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256;

    #[test]
    fn merkle_root_balanced() {
        let leaves = (0..4u64)
            .map(|v| Hash256::new(double_sha256(&v.to_le_bytes())))
            .collect::<Vec<_>>();

        let root = merkle_root(&leaves);
        assert_ne!(root, Hash256::ZERO);
    }

    #[test]
    fn merkle_root_single() {
        let hash = Hash256::new(sha256(b"neo-n3"));
        let root = merkle_root(&[hash]);
        assert_eq!(root, hash);
    }
}
