/// Convenience helper to compute Hash160 = RIPEMD160(SHA256(data)).
#[inline]
pub fn hash160(message: &[u8]) -> [u8; 20] {
    neo_base::hash::hash160(message)
}

/// Convenience helper to compute double SHA-256.
#[inline]
pub fn hash256(message: &[u8]) -> [u8; 32] {
    neo_base::hash::double_sha256(message)
}
