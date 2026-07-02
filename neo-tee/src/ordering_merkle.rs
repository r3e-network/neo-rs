//! Internal Merkle helper for TEE ordering proofs.
//!
//! This is not the Neo block Merkle tree. Ordering proofs use single
//! `SHA-256(left || right)` parent hashes over raw 32-byte transaction hashes,
//! duplicating the last leaf at odd levels. The producer and verifier both use
//! this helper so proof semantics cannot drift.

/// Computes the Merkle root for ordered TEE transaction hashes.
#[must_use]
pub(crate) fn ordering_merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }

    let mut level: Vec<[u8; 32]> = hashes.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for chunk in level.chunks(2) {
            let right = chunk.get(1).unwrap_or(&chunk[0]);
            next.push(hash_pair(&chunk[0], right));
        }
        level = next;
    }

    level[0]
}

fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut data = [0u8; 64];
    data[..32].copy_from_slice(left);
    data[32..].copy_from_slice(right);
    neo_crypto::Crypto::sha256(&data)
}

#[cfg(test)]
mod tests {
    use super::ordering_merkle_root;

    #[test]
    fn empty_hash_list_returns_zero_root() {
        assert_eq!(ordering_merkle_root(&[]), [0u8; 32]);
    }

    #[test]
    fn single_hash_is_its_own_root() {
        let hash = [7u8; 32];
        assert_eq!(ordering_merkle_root(&[hash]), hash);
    }

    #[test]
    fn odd_levels_duplicate_last_leaf() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];

        let ab = pair_root(a, b);
        let cc = pair_root(c, c);
        let expected = pair_root(ab, cc);

        assert_eq!(ordering_merkle_root(&[a, b, c]), expected);
    }

    fn pair_root(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
        let mut data = [0u8; 64];
        data[..32].copy_from_slice(&left);
        data[32..].copy_from_slice(&right);
        neo_crypto::Crypto::sha256(&data)
    }
}
