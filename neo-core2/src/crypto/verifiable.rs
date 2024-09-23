pub mod crypto;

use crate::crypto::hash::Hashable;

// VerifiableDecodable represents an object which can be verified and
// those hashable part of which can be encoded/decoded.
pub trait VerifiableDecodable: Hashable {
    fn encode_hashable_fields(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
    fn decode_hashable_fields(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
}
