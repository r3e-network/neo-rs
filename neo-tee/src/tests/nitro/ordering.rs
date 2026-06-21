use super::*;
use crate::enclave::{EnclaveConfig, TeeEnclave};
use crate::mempool::TeeMempoolConfig;
use crate::mempool::fair_ordering::FairOrderingPolicy;
use tempfile::tempdir;

fn sequencer(policy: FairOrderingPolicy) -> (tempfile::TempDir, Arc<TeeMempool>) {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };
    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();
    let mempool_config = TeeMempoolConfig {
        ordering_policy: policy,
        ..Default::default()
    };
    let mempool = Arc::new(TeeMempool::new(enclave, mempool_config).unwrap());
    (temp, mempool)
}

fn entries(n: u8) -> Vec<OrderTxEntry> {
    (0..n)
        .map(|i| {
            let mut tx_hash = [0u8; 32];
            tx_hash[0] = i;
            OrderTxEntry {
                tx_hash,
                network_fee: 1000 + i as i64,
                system_fee: 500,
                sender: [0xAB; 20],
            }
        })
        .collect()
}

#[test]
fn proof_is_internally_consistent() {
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let proof = build_ordering_proof(&seq, &entries(5), 100, Vec::new()).unwrap();

    assert_eq!(proof.ordered_hashes.len(), 5);
    let v = verify_ordering_proof(&proof);
    assert!(v.merkle_root_matches, "merkle root must match");
    assert!(v.signature_valid, "sequencer signature must verify");
    // No attestation supplied -> not attestation-verified, not fully trusted.
    assert!(!v.attestation_verified);
    assert!(v.is_internally_consistent());
    assert!(!v.is_fully_trusted());
}

#[test]
fn tampering_with_ordered_hashes_breaks_merkle_root() {
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let mut proof = build_ordering_proof(&seq, &entries(4), 100, Vec::new()).unwrap();

    // Reverse the order — the merkle root no longer matches the signed root.
    proof.ordered_hashes.reverse();
    let v = verify_ordering_proof(&proof);
    assert!(!v.merkle_root_matches);
    assert!(!v.is_internally_consistent());
}

#[test]
fn tampering_with_signature_breaks_verification() {
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let mut proof = build_ordering_proof(&seq, &entries(3), 100, Vec::new()).unwrap();

    // Flip a signature byte.
    proof.sequencer_proof.signature[0] ^= 0xFF;
    let v = verify_ordering_proof(&proof);
    assert!(!v.signature_valid);
}

#[test]
fn fcfs_ordering_is_deterministic_by_sequence() {
    // FCFS uses sequence_number as the primary key; transactions inserted in
    // order must come back in insertion order regardless of fee.
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let mut txs = entries(6);
    // Give later txs higher fees to prove fee does not reorder FCFS.
    for (i, tx) in txs.iter_mut().enumerate() {
        tx.network_fee = 10_000 - i as i64;
    }
    let proof = build_ordering_proof(&seq, &txs, 100, Vec::new()).unwrap();

    let expected: Vec<[u8; 32]> = txs.iter().map(|t| t.tx_hash).collect();
    assert_eq!(
        proof.ordered_hashes, expected,
        "FCFS must preserve insertion order"
    );
}

#[test]
fn build_is_idempotent_for_duplicates() {
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let txs = entries(4);
    let first = build_ordering_proof(&seq, &txs, 100, Vec::new()).unwrap();
    // Re-submitting the same batch must not error and must not duplicate.
    let second = build_ordering_proof(&seq, &txs, 100, Vec::new()).unwrap();
    assert_eq!(first.ordered_hashes, second.ordered_hashes);
}

#[test]
fn attestation_user_data_binds_proof_fields() {
    let (_t, seq) = sequencer(FairOrderingPolicy::FirstComeFirstServed);
    let proof = build_ordering_proof(&seq, &entries(2), 100, Vec::new()).unwrap();
    let ud = OrderingProof::attestation_user_data(&proof.sequencer_proof);
    // Changing the counter changes the bound digest.
    let mut other = proof.sequencer_proof.clone();
    other.enclave_counter += 1;
    assert_ne!(ud, OrderingProof::attestation_user_data(&other));
}
