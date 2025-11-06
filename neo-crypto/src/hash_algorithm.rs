use alloc::vec::Vec;

use neo_base::hash;

/// Hash algorithms supported by Neo's signing primitives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashAlgorithm {
    Sha256,
    Keccak256,
    Sha512,
}

impl HashAlgorithm {
    /// Hash `message` according to the configured algorithm.
    #[inline]
    pub fn digest(self, message: &[u8]) -> Vec<u8> {
        match self {
            HashAlgorithm::Sha256 => hash::sha256(message).to_vec(),
            HashAlgorithm::Keccak256 => hash::keccak256(message).to_vec(),
            HashAlgorithm::Sha512 => hash::sha512(message).to_vec(),
        }
    }
}
