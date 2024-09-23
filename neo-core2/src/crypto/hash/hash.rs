use sha2::{Sha256, Digest};
use ripemd160::Ripemd160;
use byteorder::{ByteOrder, LittleEndian};

use crate::util::{Uint256, Uint160};

// Hashable represents an object which can be hashed. Usually, these objects
// are io.Serializable and signable. They tend to cache the hash inside for
// effectiveness, providing this accessor method. Anything that can be
// identified with a hash can then be signed and verified.
pub trait Hashable {
    fn hash(&self) -> Uint256;
}

// GetSignedData returns the concatenated byte slice containing of the network
// magic in constant-length 4-bytes LE representation and hashable item hash in BE
// representation.
pub fn get_signed_data(net: u32, hh: &dyn Hashable) -> Vec<u8> {
    let mut b = vec![0u8; 4 + Uint256::size()];
    LittleEndian::write_u32(&mut b, net);
    let h = hh.hash();
    b[4..].copy_from_slice(&h.to_bytes_be());
    b
}

// NetSha256 calculates a network-specific hash of the Hashable item that can then
// be signed/verified.
pub fn net_sha256(net: u32, hh: &dyn Hashable) -> Uint256 {
    sha256(&get_signed_data(net, hh))
}

// Sha256 hashes the incoming byte slice
// using the sha256 algorithm.
pub fn sha256(data: &[u8]) -> Uint256 {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    Uint256::from_bytes_be(&result)
}

// DoubleSha256 performs sha256 twice on the given data.
pub fn double_sha256(data: &[u8]) -> Uint256 {
    let h1 = sha256(data);
    sha256(&h1.to_bytes_be())
}

// RipeMD160 performs the RIPEMD160 hash algorithm
// on the given data.
pub fn ripemd160(data: &[u8]) -> Uint160 {
    let mut hasher = Ripemd160::new();
    hasher.update(data);
    let result = hasher.finalize();
    Uint160::from_bytes_be(&result)
}

// Hash160 performs sha256 and then ripemd160
// on the given data.
pub fn hash160(data: &[u8]) -> Uint160 {
    let h1 = sha256(data);
    ripemd160(&h1.to_bytes_be())
}

// Checksum returns the checksum for a given piece of data
// using DoubleSha256 as the hash algorithm. It returns the
// first 4 bytes of the resulting slice.
pub fn checksum(data: &[u8]) -> Vec<u8> {
    let hash = double_sha256(data);
    hash.to_bytes_be()[..4].to_vec()
}
