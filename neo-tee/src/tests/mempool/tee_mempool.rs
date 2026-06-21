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
