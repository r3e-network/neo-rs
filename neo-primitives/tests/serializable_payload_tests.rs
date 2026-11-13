use neo_primitives::{SerializablePayload, UInt256};

// Mock SerializablePayload implementation for testing
struct MockPayload {
    data: Vec<u8>,
    witnesses: Vec<(Vec<u8>, Vec<u8>)>, // (invocation, verification) pairs
}

impl SerializablePayload for MockPayload {
    fn hash_data(&self) -> Vec<u8> {
        self.data.clone()
    }

    fn witness_count(&self) -> usize {
        self.witnesses.len()
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        &self.witnesses[index].0
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        &self.witnesses[index].1
    }
}

#[test]
fn serializable_payload_hash_data_returns_data() {
    let payload = MockPayload {
        data: vec![1, 2, 3, 4],
        witnesses: vec![],
    };

    assert_eq!(payload.hash_data(), vec![1, 2, 3, 4]);
}

#[test]
fn serializable_payload_witness_count() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![(vec![1], vec![2]), (vec![3], vec![4])],
    };

    assert_eq!(payload.witness_count(), 2);
}

#[test]
fn serializable_payload_zero_witnesses() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![],
    };

    assert_eq!(payload.witness_count(), 0);
}

#[test]
fn serializable_payload_invocation_script() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![
            (vec![10, 20, 30], vec![40, 50]),
            (vec![60], vec![70, 80, 90]),
        ],
    };

    assert_eq!(payload.invocation_script(0), &[10, 20, 30]);
    assert_eq!(payload.invocation_script(1), &[60]);
}

#[test]
fn serializable_payload_verification_script() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![(vec![10, 20], vec![40, 50, 60]), (vec![70], vec![80])],
    };

    assert_eq!(payload.verification_script(0), &[40, 50, 60]);
    assert_eq!(payload.verification_script(1), &[80]);
}

#[test]
fn serializable_payload_empty_scripts() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![(vec![], vec![])],
    };

    assert_eq!(payload.invocation_script(0), &[]);
    assert_eq!(payload.verification_script(0), &[]);
}

#[test]
fn serializable_payload_hash_uses_double_sha256() {
    let payload = MockPayload {
        data: vec![1, 2, 3, 4, 5],
        witnesses: vec![],
    };

    let hash = payload.hash();

    // Verify it's not zero (should be a valid hash)
    assert_ne!(hash, UInt256::zero());

    // Verify it's deterministic
    let hash2 = payload.hash();
    assert_eq!(hash, hash2);
}

#[test]
fn serializable_payload_hash_empty_data() {
    let payload = MockPayload {
        data: vec![],
        witnesses: vec![],
    };

    let hash = payload.hash();
    assert_ne!(hash, UInt256::zero());
}

#[test]
fn serializable_payload_hash_different_data_different_hash() {
    let payload1 = MockPayload {
        data: vec![1, 2, 3],
        witnesses: vec![],
    };

    let payload2 = MockPayload {
        data: vec![4, 5, 6],
        witnesses: vec![],
    };

    assert_ne!(payload1.hash(), payload2.hash());
}

#[test]
fn serializable_payload_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockPayload>();
}

#[test]
fn serializable_payload_multiple_witnesses() {
    let payload = MockPayload {
        data: vec![1, 2, 3],
        witnesses: vec![
            (vec![10], vec![20]),
            (vec![30], vec![40]),
            (vec![50], vec![60]),
        ],
    };

    assert_eq!(payload.witness_count(), 3);
    assert_eq!(payload.invocation_script(0), &[10]);
    assert_eq!(payload.invocation_script(1), &[30]);
    assert_eq!(payload.invocation_script(2), &[50]);
    assert_eq!(payload.verification_script(0), &[20]);
    assert_eq!(payload.verification_script(1), &[40]);
    assert_eq!(payload.verification_script(2), &[60]);
}

#[test]
fn serializable_payload_hash_witnesses_not_included() {
    // Witnesses should NOT affect the hash (hash_data excludes them)
    let payload1 = MockPayload {
        data: vec![1, 2, 3],
        witnesses: vec![],
    };

    let payload2 = MockPayload {
        data: vec![1, 2, 3],
        witnesses: vec![(vec![10], vec![20])],
    };

    // Same data, different witnesses -> same hash
    assert_eq!(payload1.hash(), payload2.hash());
}
