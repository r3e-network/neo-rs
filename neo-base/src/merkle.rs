// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use alloc::vec;

use crate::hash::{Sha256, SlicesSha256};

pub trait MerkleSha256 {
    /// return the root hash of the merkle tree, in little endian
    fn merkle_sha256(&self) -> [u8; 32];
}

impl<T: AsRef<[[u8; 32]]>> MerkleSha256 for T {
    fn merkle_sha256(&self) -> [u8; 32] {
        let hashes = self.as_ref();
        if hashes.len() == 0 {
            return [0u8; 32];
        }

        if hashes.len() == 1 {
            return hashes[0];
        }

        let mut nodes = vec![[0u8; 32]; (hashes.len() + 1) / 2];
        for k in 0..nodes.len() {
            nodes[k] = children_sha256(2 * k, hashes);
        }

        let mut prev = nodes.len();
        let mut right = (nodes.len() + 1) / 2;
        while prev > right {
            for k in 0..right {
                nodes[k] = children_sha256(2 * k, &nodes[..prev]);
            }

            prev = right;
            right = (right + 1) / 2;
        }

        nodes[0]
    }
}

#[inline]
fn children_sha256(off: usize, hashes: &[[u8; 32]]) -> [u8; 32] {
    let two = if off + 1 >= hashes.len() {
        [&hashes[off], &hashes[off]]
    } else {
        [&hashes[off], &hashes[off + 1]]
    };

    two.iter().slices_sha256().sha256()
}
