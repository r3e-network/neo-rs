//! Integration tests for the encrypted mempool core (Privacy Mode Phase A,
//! task 50). These exercise the public crate API only: encrypted intake,
//! host-visible ciphertext-only storage, the decrypt-first state machine,
//! post-decryption ordering, and ordering proofs — for both KEM modes
//! (simulated committee epoch key and enclave-sealed key).
//!
//! Simulation caveat: under the `simulation` feature these tests verify the
//! interfaces and flow, not real enclave isolation.

use neo_tee::enclave::{EnclaveConfig, TeeEnclave};
use neo_tee::mempool::{
    CIPHERTEXT_VERSION, ContentEncryptionConfig, ContentState, EncryptionMode, FairOrderingPolicy,
    MODE_COMMITTEE_KEY, MODE_TEE_SEALED_KEY, MempoolCiphertext, ResolveSummary, TeeMempool,
    TeeMempoolConfig,
};
use std::sync::Arc;
use tempfile::tempdir;

fn setup_enclave() -> (tempfile::TempDir, Arc<TeeEnclave>) {
    let temp = tempdir().expect("temp dir");
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };
    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().expect("enclave init");
    (temp, enclave)
}

fn plaintext_mempool() -> (tempfile::TempDir, TeeMempool) {
    let (temp, enclave) = setup_enclave();
    let mempool = TeeMempool::new(enclave, TeeMempoolConfig::default()).expect("mempool init");
    (temp, mempool)
}

/// Returns the temp dir alongside the mempool: the TeeSealedKey KEM keeps
/// writing floor/counter witness files into the enclave's sealed-data
/// directory, so the directory must outlive the pool.
fn encrypted_mempool(
    mode: EncryptionMode,
    policy: FairOrderingPolicy,
) -> (tempfile::TempDir, TeeMempool) {
    let (temp, enclave) = setup_enclave();
    let mempool = TeeMempool::with_content_encryption(
        enclave,
        TeeMempoolConfig {
            ordering_policy: policy,
            ..Default::default()
        },
        ContentEncryptionConfig { mode },
    )
    .expect("mempool init");
    (temp, mempool)
}

fn sample_txs(count: usize, prefix: &str) -> Vec<([u8; 32], Vec<u8>)> {
    (0..count)
        .map(|i| {
            let mut hash = [0u8; 32];
            // Spread bytes so hashes are distinct and non-trivial.
            hash[0] = (i + 1) as u8;
            hash[31] = 0xA0 ^ (i as u8);
            (hash, format!("{prefix}-payload-{i}").into_bytes())
        })
        .collect()
}

fn add_all(mempool: &TeeMempool, txs: &[([u8; 32], Vec<u8>)]) {
    let sender = [0xABu8; 20];
    for (hash, payload) in txs {
        mempool
            .add_transaction(*hash, payload.clone(), 1000, 500, sender)
            .expect("add transaction");
    }
}

#[test]
fn default_off_keeps_stored_bytes_identical_to_plaintext() {
    let (_temp, mempool) = plaintext_mempool();
    let txs = sample_txs(4, "default");

    add_all(&mempool, &txs);

    // The default configuration stores and returns the exact plaintext bytes.
    for (hash, payload) in &txs {
        assert_eq!(mempool.get_transaction(hash).as_ref(), Some(payload));
        assert_eq!(mempool.content_state(hash), Some(ContentState::Resolved));
    }

    // Candidate sets carry the original bytes.
    let ordered = mempool.get_ordered_transactions(usize::MAX);
    assert_eq!(ordered.len(), txs.len());
    let by_hash: std::collections::HashMap<[u8; 32], Vec<u8>> = ordered.into_iter().collect();
    for (hash, payload) in &txs {
        assert_eq!(by_hash.get(hash), Some(payload));
    }

    // Decrypt stage is a no-op; nothing is ever rejected.
    assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
    assert!(mempool.rejected_hashes().is_empty());

    // Ordering proof API works unchanged.
    let proof = mempool.generate_ordering_proof().expect("proof");
    assert_ne!(proof.merkle_root, [0u8; 32]);
    assert!(!proof.signature.is_empty());
}

#[test]
fn committee_mode_host_view_is_ciphertext_only() {
    let (_temp, mempool) = encrypted_mempool(
        EncryptionMode::CommitteeKey { epoch_id: 7 },
        FairOrderingPolicy::ThresholdEncryption,
    );
    let txs = sample_txs(3, "committee");
    add_all(&mempool, &txs);

    for (hash, payload) in &txs {
        let stored = mempool.get_transaction(hash).expect("stored envelope");

        // Not the plaintext, and the plaintext bytes are not recoverable
        // from anywhere in the host-visible storage.
        assert_ne!(&stored, payload);
        assert!(
            !stored
                .windows(payload.len())
                .any(|w| w == payload.as_slice())
        );

        // The stored bytes parse as a valid ciphertext envelope.
        let decoded = MempoolCiphertext::decode(&stored).expect("valid envelope");
        assert_eq!(decoded.version, CIPHERTEXT_VERSION);
        assert_eq!(decoded.mode, MODE_COMMITTEE_KEY);
        assert_eq!(decoded.epoch_id, 7);

        // Decrypt-first state machine: still Encrypted, no plaintext out.
        assert_eq!(mempool.content_state(hash), Some(ContentState::Encrypted));
        assert_eq!(mempool.get_resolved_transaction(hash), None);
    }

    // No candidates before the decrypt stage runs.
    assert!(mempool.get_ordered_hashes(usize::MAX).is_empty());
    assert!(mempool.get_ordered_transactions(usize::MAX).is_empty());
}

#[test]
fn committee_mode_decrypt_then_ordered_candidates_and_proof() {
    let (_temp, mempool) = encrypted_mempool(
        EncryptionMode::CommitteeKey { epoch_id: 7 },
        FairOrderingPolicy::ThresholdEncryption,
    );
    let txs = sample_txs(4, "order");
    add_all(&mempool, &txs);

    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 4,
            rejected: 0
        }
    );

    // Threshold-encryption ordering is assigned AFTER decryption and follows
    // the arrival sequence decided at intake.
    let ordered_hashes = mempool.get_ordered_hashes(usize::MAX);
    let want: Vec<[u8; 32]> = txs.iter().map(|(h, _)| *h).collect();
    assert_eq!(ordered_hashes, want);

    // Decrypted payloads are surfaced in the same order.
    let recovered = mempool.get_ordered_transactions(usize::MAX);
    for ((hash, data), (want_hash, want_payload)) in recovered.iter().zip(&txs) {
        assert_eq!(hash, want_hash);
        assert_eq!(data, want_payload);
    }

    // The ordering proof covers the resolved ordered-hash table.
    let proof = mempool.generate_ordering_proof().expect("proof");
    assert_ne!(proof.merkle_root, [0u8; 32]);

    // A second decrypt pass changes nothing (idempotent).
    assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
    assert_eq!(mempool.get_ordered_hashes(usize::MAX), want);
}

#[test]
fn tee_sealed_mode_round_trip_via_public_api() {
    let (_temp, mempool) = encrypted_mempool(
        EncryptionMode::TeeSealedKey,
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let txs = sample_txs(3, "sealed");
    add_all(&mempool, &txs);

    for (hash, payload) in &txs {
        let stored = mempool.get_transaction(hash).expect("stored envelope");
        assert_ne!(&stored, payload);
        assert!(
            !stored
                .windows(payload.len())
                .any(|w| w == payload.as_slice())
        );
        assert_eq!(
            MempoolCiphertext::decode(&stored).expect("envelope").mode,
            MODE_TEE_SEALED_KEY
        );
    }

    let summary = mempool.resolve_pending();
    assert_eq!(
        summary,
        ResolveSummary {
            resolved: 3,
            rejected: 0
        }
    );

    // FCFS ordering by arrival sequence, payloads recovered exactly.
    let want: Vec<[u8; 32]> = txs.iter().map(|(h, _)| *h).collect();
    assert_eq!(mempool.get_ordered_hashes(usize::MAX), want);
    for (hash, payload) in &txs {
        assert_eq!(
            mempool.get_resolved_transaction(hash),
            Some(payload.clone())
        );
    }

    let proof = mempool.generate_ordering_proof().expect("proof");
    assert_ne!(proof.merkle_root, [0u8; 32]);
}

#[test]
fn limit_respects_resolved_only_and_resolves_nothing_new() {
    let (_temp, mempool) = encrypted_mempool(
        EncryptionMode::CommitteeKey { epoch_id: 1 },
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let txs = sample_txs(5, "limit");
    add_all(&mempool, &txs);

    // Nothing resolved yet: even a large limit yields no candidates.
    assert_eq!(mempool.get_ordered_hashes(3), Vec::<[u8; 32]>::new());

    mempool.resolve_pending();

    // Limits count Resolved candidates, not raw index entries.
    let limited = mempool.get_ordered_hashes(2);
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0], txs[0].0);
    assert_eq!(limited[1], txs[1].0);

    let limited_txs = mempool.get_ordered_transactions(2);
    assert_eq!(limited_txs.len(), 2);
    assert_eq!(limited_txs[0].1, txs[0].1);
    assert_eq!(limited_txs[1].1, txs[1].1);
}

#[test]
fn remove_transaction_works_in_encrypted_mode() {
    let (_temp, mempool) = encrypted_mempool(
        EncryptionMode::TeeSealedKey,
        FairOrderingPolicy::FirstComeFirstServed,
    );
    let txs = sample_txs(2, "remove");
    add_all(&mempool, &txs);
    mempool.resolve_pending();

    assert!(mempool.remove_transaction(&txs[0].0));
    assert!(!mempool.contains(&txs[0].0));
    assert_eq!(mempool.get_ordered_hashes(usize::MAX), vec![txs[1].0]);
    assert_eq!(mempool.len(), 1);
}
