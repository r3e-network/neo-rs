use super::*;
use sha2::{Digest, Sha256};

struct DummyPayload(Vec<u8>);

impl SerializablePayload for DummyPayload {
    fn hash_data(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn witness_count(&self) -> usize {
        0
    }

    fn invocation_script(&self, _index: usize) -> &[u8] {
        &[]
    }

    fn verification_script(&self, _index: usize) -> &[u8] {
        &[]
    }
}

#[test]
fn default_hash_is_single_sha256_of_unsigned_data() {
    let payload = DummyPayload(b"neo-n3-payload".to_vec());
    let first = Sha256::digest(payload.hash_data());
    let second = Sha256::digest(first.as_slice());

    assert_eq!(payload.hash(), UInt256::from_bytes(&first).unwrap());
    assert_ne!(payload.hash(), UInt256::from_bytes(&second).unwrap());
}
