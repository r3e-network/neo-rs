use super::*;
use crate::enclave::EnclaveConfig;
use tempfile::tempdir;

fn setup_mempool() -> (tempfile::TempDir, TeeMempool) {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();

    let mempool_config = TeeMempoolConfig::default();
    let mempool = TeeMempool::new(enclave, mempool_config).unwrap();

    (temp, mempool)
}

#[test]
fn test_add_and_retrieve_transactions() {
    let (_temp, mempool) = setup_mempool();

    let tx1_hash = [1u8; 32];
    let tx1_data = vec![0x01, 0x02, 0x03];
    let sender = [0xABu8; 20];

    let seq = mempool
        .add_transaction(tx1_hash, tx1_data.clone(), 1000, 500, sender)
        .unwrap();
    assert_eq!(seq, 1);

    let tx2_hash = [2u8; 32];
    let tx2_data = vec![0x04, 0x05, 0x06];

    let seq = mempool
        .add_transaction(tx2_hash, tx2_data.clone(), 2000, 500, sender)
        .unwrap();
    assert_eq!(seq, 2);

    assert_eq!(mempool.len(), 2);
    assert!(mempool.contains(&tx1_hash));
    assert!(mempool.contains(&tx2_hash));
}

#[test]
fn test_fair_ordering() {
    let (_temp, mempool) = setup_mempool();
    let sender = [0xABu8; 20];

    // Add transactions
    for i in 0..10 {
        let mut hash = [0u8; 32];
        hash[0] = i;
        mempool
            .add_transaction(hash, vec![i], 1000, 500, sender)
            .unwrap();
    }

    let ordered = mempool.get_ordered_hashes(10);
    assert_eq!(ordered.len(), 10);

    // With FCFS policy, first transaction should come first
    // (Note: actual ordering depends on policy and randomness)
}

#[test]
fn test_ordering_proof() {
    let (_temp, mempool) = setup_mempool();
    let sender = [0xABu8; 20];

    for i in 0..5 {
        let mut hash = [0u8; 32];
        hash[0] = i;
        mempool
            .add_transaction(hash, vec![i], 1000, 500, sender)
            .unwrap();
    }

    let proof = mempool.generate_ordering_proof().unwrap();
    assert_ne!(proof.merkle_root, [0u8; 32]);
    assert!(!proof.signature.is_empty());
}

#[test]
fn test_capacity_limit() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();

    let mempool_config = TeeMempoolConfig {
        capacity: 5,
        ..Default::default()
    };
    let mempool = TeeMempool::new(enclave, mempool_config).unwrap();
    let sender = [0xABu8; 20];

    // Fill to capacity
    for i in 0..5 {
        let mut hash = [0u8; 32];
        hash[0] = i;
        mempool
            .add_transaction(hash, vec![i], 1000, 500, sender)
            .unwrap();
    }

    // Should fail at capacity
    let mut hash = [0u8; 32];
    hash[0] = 100;
    let result = mempool.add_transaction(hash, vec![100], 1000, 500, sender);
    assert!(matches!(result, Err(TeeError::MempoolFull)));
}

// ---- Privacy Mode Phase A: encrypted mempool tests ----

fn setup_enclave(temp: &tempfile::TempDir) -> Arc<TeeEnclave> {
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };
    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();
    enclave
}

fn setup_mempool_with(
    mode: EncryptionMode,
    policy: FairOrderingPolicy,
) -> (tempfile::TempDir, TeeMempool) {
    let temp = tempdir().unwrap();
    let enclave = setup_enclave(&temp);
    let mempool = TeeMempool::with_content_encryption(
        enclave,
        TeeMempoolConfig {
            ordering_policy: policy,
            ..Default::default()
        },
        ContentEncryptionConfig { mode },
    )
    .unwrap();
    (temp, mempool)
}

/// Flip one byte of the stored (host-visible) data of a pool entry.
fn tamper_stored_data(mempool: &TeeMempool, tx_hash: &[u8; 32], byte_index: usize) {
    let mut transactions = mempool.transactions.write();
    let entry = transactions.get_mut(tx_hash).expect("entry must exist");
    let index = byte_index % entry.data.len();
    entry.data[index] ^= 0xFF;
}

/// Transplant a ciphertext produced by another mempool into `target`,
/// simulating a replay/foreign-blob arriving over the wire.
fn inject_foreign_ciphertext(target: &TeeMempool, source: &TeeMempool, tx_hash: &[u8; 32]) {
    let data = source.get_transaction(tx_hash).expect("source entry");
    let timing = TransactionTiming::new(1);
    let ordering_key = compute_ordering_key(
        FairOrderingPolicy::ThresholdEncryption,
        &timing,
        tx_hash,
        0,
        0,
    );
    let entry = TeeMempoolEntry {
        hash: *tx_hash,
        data,
        timing,
        ordering_key: ordering_key.clone(),
        network_fee: 0,
        system_fee: 0,
        sender: [0u8; 20],
        content_state: ContentState::Encrypted,
        resolved_data: None,
    };
    target.ordered.write().insert(ordering_key, *tx_hash);
    target.transactions.write().insert(*tx_hash, entry);
}

#[test]
fn test_default_mode_stores_plaintext_bytes_identically() {
    let (_temp, mempool) = setup_mempool();
    // Strictly opt-in: the default pool has no encryption engine at all.
    assert!(mempool.content_encryption.is_none());
    let sender = [0xABu8; 20];

    let payloads: Vec<([u8; 32], Vec<u8>)> = (0..3)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0] = i;
            (hash, format!("plaintext-payload-{i}").into_bytes())
        })
        .collect();

    for (hash, payload) in &payloads {
        mempool
            .add_transaction(*hash, payload.clone(), 1000, 500, sender)
            .unwrap();
    }

    // Stored bytes are byte-for-byte identical to the input plaintext.
    for (hash, payload) in &payloads {
        assert_eq!(mempool.get_transaction(hash).as_ref(), Some(payload));
        assert_eq!(mempool.content_state(hash), Some(ContentState::Resolved));
        assert_eq!(
            mempool.get_resolved_transaction(hash),
            Some(payload.clone())
        );
    }

    // Candidate sets return the exact original bytes. (The default
    // policy is BatchedRandom, so ordering is random — compare by hash.)
    let ordered = mempool.get_ordered_transactions(10);
    assert_eq!(ordered.len(), 3);
    let by_hash: HashMap<[u8; 32], Vec<u8>> = ordered.into_iter().collect();
    for (hash, payload) in &payloads {
        assert_eq!(by_hash.get(hash), Some(payload));
    }

    // resolve_pending is a no-op in plaintext mode.
    assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
    assert!(mempool.rejected_hashes().is_empty());

    // Ordering proof still works unchanged.
    let proof = mempool.generate_ordering_proof().unwrap();
    assert_ne!(proof.merkle_root, [0u8; 32]);
}

#[test]
fn test_committee_mode_host_sees_only_ciphertext_then_orders_after_decrypt() {
    let (_temp, mempool) = setup_mempool_with(
        EncryptionMode::CommitteeKey { epoch_id: 7 },
        FairOrderingPolicy::ThresholdEncryption,
    );
    let sender = [0xABu8; 20];

    let payloads: Vec<([u8; 32], Vec<u8>)> = (0..3)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0] = i;
            (hash, format!("committee-payload-{i}").into_bytes())
        })
        .collect();
    for (hash, payload) in &payloads {
        mempool
            .add_transaction(*hash, payload.clone(), 1000, 500, sender)
            .unwrap();
    }

    // Host-visible storage: ciphertext envelope only, no plaintext bytes.
    for (hash, payload) in &payloads {
        let stored = mempool.get_transaction(hash).expect("stored envelope");
        assert_ne!(&stored, payload);
        assert!(
            !stored
                .windows(payload.len())
                .any(|w| w == payload.as_slice())
        );

        let decoded = MempoolCiphertext::decode(&stored).unwrap();
        assert_eq!(decoded.mode, crate::mempool::crypto::MODE_COMMITTEE_KEY);
        assert_eq!(decoded.epoch_id, 7);

        assert_eq!(mempool.content_state(hash), Some(ContentState::Encrypted));
        assert_eq!(mempool.get_resolved_transaction(hash), None);
    }

    // Pre-resolution candidate sets are empty (decrypt-first state machine).
    assert!(mempool.get_ordered_hashes(10).is_empty());
    assert!(mempool.get_ordered_transactions(10).is_empty());

    // Decrypt stage.
    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 3,
            rejected: 0
        }
    );

    // Ordering was assigned AFTER decryption (arrival sequence).
    let ordered_hashes = mempool.get_ordered_hashes(10);
    let want_hashes: Vec<[u8; 32]> = payloads.iter().map(|(h, _)| *h).collect();
    assert_eq!(ordered_hashes, want_hashes);

    // Decrypted payloads come back in the same order, byte-identical.
    let recovered = mempool.get_ordered_transactions(10);
    for ((hash, data), (want_hash, want_payload)) in recovered.iter().zip(&payloads) {
        assert_eq!(hash, want_hash);
        assert_eq!(data, want_payload);
    }

    for (hash, payload) in &payloads {
        assert_eq!(mempool.content_state(hash), Some(ContentState::Resolved));
        assert_eq!(
            mempool.get_resolved_transaction(hash),
            Some(payload.clone())
        );
    }

    // OrderingProof covers the resolved ordered-hash table.
    let proof = mempool.generate_ordering_proof().unwrap();
    assert_ne!(proof.merkle_root, [0u8; 32]);

    // Resolution is idempotent: a second pass resolves nothing.
    assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
}

#[test]
fn test_tee_sealed_mode_round_trip() {
    let (_temp, mempool) = setup_mempool_with(
        EncryptionMode::TeeSealedKey,
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let sender = [0xABu8; 20];

    let mut hash_a = [0x11u8; 32];
    hash_a[0] = 1;
    let mut hash_b = [0x22u8; 32];
    hash_b[0] = 2;
    let payload_a = b"tee-sealed-payload-a".to_vec();
    let payload_b = b"tee-sealed-payload-b".to_vec();

    mempool
        .add_transaction(hash_a, payload_a.clone(), 1000, 500, sender)
        .unwrap();
    mempool
        .add_transaction(hash_b, payload_b.clone(), 2000, 500, sender)
        .unwrap();

    let stored_a = mempool.get_transaction(&hash_a).unwrap();
    assert_ne!(stored_a, payload_a);
    assert!(
        !stored_a
            .windows(payload_a.len())
            .any(|w| w == payload_a.as_slice())
    );
    assert_eq!(
        MempoolCiphertext::decode(&stored_a).unwrap().mode,
        crate::mempool::crypto::MODE_TEE_SEALED_KEY
    );

    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 2,
            rejected: 0
        }
    );

    assert_eq!(mempool.get_ordered_hashes(10), vec![hash_a, hash_b]);
    assert_eq!(mempool.get_resolved_transaction(&hash_a), Some(payload_a));
    assert_eq!(mempool.get_resolved_transaction(&hash_b), Some(payload_b));
}

#[test]
fn test_tampered_ciphertext_rejected_fail_closed() {
    let (_temp, mempool) = setup_mempool_with(
        EncryptionMode::TeeSealedKey,
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let sender = [0xABu8; 20];

    let mut good_hash = [0u8; 32];
    good_hash[0] = 1;
    let mut bad_hash = [0u8; 32];
    bad_hash[0] = 2;
    let good_payload = b"good-payload".to_vec();
    let bad_payload = b"tampered-payload".to_vec();

    mempool
        .add_transaction(good_hash, good_payload.clone(), 1000, 500, sender)
        .unwrap();
    mempool
        .add_transaction(bad_hash, bad_payload, 1000, 500, sender)
        .unwrap();

    // Tamper the AEAD tag (last byte of the stored envelope).
    let stored_len = mempool.get_transaction(&bad_hash).unwrap().len();
    tamper_stored_data(&mempool, &bad_hash, stored_len - 1);

    // The decrypt stage must never panic and must fail closed.
    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 1,
            rejected: 1
        }
    );

    match mempool.content_state(&bad_hash) {
        Some(ContentState::Rejected { reason }) => {
            assert!(
                reason.contains("content decryption failed"),
                "reason: {reason}"
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    // Fail-closed: the tampered entry never enters a candidate set.
    assert_eq!(mempool.get_ordered_hashes(10), vec![good_hash]);
    assert_eq!(mempool.get_resolved_transaction(&bad_hash), None);
    assert_eq!(mempool.rejected_hashes(), vec![bad_hash]);
    assert_eq!(mempool.len(), 2);

    // Purging removes the rejected entry without touching the good one.
    assert_eq!(mempool.purge_rejected(), 1);
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&good_hash));
    assert!(!mempool.contains(&bad_hash));
    assert_eq!(mempool.get_ordered_hashes(10), vec![good_hash]);
}

#[test]
fn test_tampered_kem_material_rejected() {
    let (_temp, mempool) = setup_mempool_with(
        EncryptionMode::CommitteeKey { epoch_id: 5 },
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let sender = [0xABu8; 20];
    let tx_hash = [0xEEu8; 32];

    mempool
        .add_transaction(tx_hash, b"kem-tamper-payload".to_vec(), 1000, 500, sender)
        .unwrap();

    // Flip a byte inside the committee KEM material (offset 12 is the
    // first KEM-material byte after the fixed envelope header).
    tamper_stored_data(&mempool, &tx_hash, 12);

    let summary = mempool.resolve_pending();
    assert_eq!(summary.rejected, 1);
    assert!(matches!(
        mempool.content_state(&tx_hash),
        Some(ContentState::Rejected { .. })
    ));
    assert!(mempool.get_ordered_hashes(10).is_empty());
    assert_eq!(mempool.get_resolved_transaction(&tx_hash), None);
}

#[test]
fn test_wrong_committee_key_rejected() {
    // Two pools for the same epoch but with independently generated
    // committee keys: a ciphertext from pool A must not decrypt in pool B.
    let (_temp_a, pool_a) = setup_mempool_with(
        EncryptionMode::CommitteeKey { epoch_id: 7 },
        FairOrderingPolicy::ThresholdEncryption,
    );
    let (_temp_b, pool_b) = setup_mempool_with(
        EncryptionMode::CommitteeKey { epoch_id: 7 },
        FairOrderingPolicy::ThresholdEncryption,
    );
    // A third pool on a different epoch must reject it as well.
    let (_temp_c, pool_c) = setup_mempool_with(
        EncryptionMode::CommitteeKey { epoch_id: 8 },
        FairOrderingPolicy::ThresholdEncryption,
    );

    let tx_hash = [0x77u8; 32];
    let payload = b"wrong-key-payload".to_vec();
    pool_a
        .add_transaction(tx_hash, payload, 1000, 500, [0xABu8; 20])
        .unwrap();

    inject_foreign_ciphertext(&pool_b, &pool_a, &tx_hash);
    let summary_b = pool_b.resolve_pending();
    assert_eq!(summary_b.rejected, 1);
    assert_eq!(summary_b.resolved, 0);
    assert!(matches!(
        pool_b.content_state(&tx_hash),
        Some(ContentState::Rejected { .. })
    ));
    assert!(pool_b.get_ordered_hashes(10).is_empty());
    assert_eq!(pool_b.get_resolved_transaction(&tx_hash), None);

    inject_foreign_ciphertext(&pool_c, &pool_a, &tx_hash);
    let summary_c = pool_c.resolve_pending();
    assert_eq!(summary_c.rejected, 1);
    assert!(pool_c.get_ordered_hashes(10).is_empty());
}

#[test]
fn test_tee_sealed_floor_rollback_rejected() {
    let (temp, mempool) = setup_mempool_with(
        EncryptionMode::TeeSealedKey,
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let sender = [0xABu8; 20];

    let mut hash_a = [0u8; 32];
    hash_a[0] = 1;
    let mut hash_b = [0u8; 32];
    hash_b[0] = 2;
    mempool
        .add_transaction(hash_a, b"floor-a".to_vec(), 1000, 500, sender)
        .unwrap();
    mempool
        .add_transaction(hash_b, b"floor-b".to_vec(), 1000, 500, sender)
        .unwrap();

    // Sanity: decapsulation works with an intact floor.
    let before = mempool.resolve_pending();
    assert_eq!(before.resolved, 2);

    // Roll the external floor witness back below the sealed-blob
    // counters: the blobs are no longer covered by floor history.
    let floor_path = crate::enclave::rollback_floor::floor_path(temp.path());
    crate::enclave::rollback_floor::store(&floor_path, 0).unwrap();

    // Reset both entries to Encrypted so the decrypt stage runs again
    // against the rolled-back floor (simulates a restart + replay).
    {
        let mut transactions = mempool.transactions.write();
        for entry in transactions.values_mut() {
            entry.content_state = ContentState::Encrypted;
            entry.resolved_data = None;
        }
    }

    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 0,
            rejected: 2
        }
    );
    for hash in [hash_a, hash_b] {
        match mempool.content_state(&hash) {
            Some(ContentState::Rejected { reason }) => {
                assert!(reason.contains("Rollback protection"), "reason: {reason}");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
    assert!(mempool.get_ordered_hashes(10).is_empty());
    assert_eq!(mempool.rejected_hashes().len(), 2);
}
