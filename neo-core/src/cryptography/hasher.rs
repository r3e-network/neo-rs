/// Represents hash function identifiers supported by ECDSA message signature and verification.
#[repr(u8)]
pub enum Hasher {
    /// The SHA256 hash algorithm.
    SHA256 = 0x00,

    /// The Keccak256 hash algorithm.
    Keccak256 = 0x01,
}
