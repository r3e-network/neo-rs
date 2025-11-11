use crate::hash_algorithm::HashAlgorithm;

pub(crate) fn derive_prehash(message: &[u8], algorithm: HashAlgorithm) -> [u8; 32] {
    let digest = algorithm.digest(message);
    let mut arr = [0u8; 32];
    if digest.len() >= 32 {
        arr.copy_from_slice(&digest[..32]);
    } else {
        let offset = 32 - digest.len();
        arr[offset..].copy_from_slice(&digest);
    }
    arr
}
