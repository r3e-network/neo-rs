use super::*;

#[test]
fn test_fcfs_ordering() {
    let policy = FairOrderingPolicy::FirstComeFirstServed;

    let timing1 = TransactionTiming::new(1);
    let timing2 = TransactionTiming::new(2);

    let hash1 = [0u8; 32];
    let hash2 = [1u8; 32];

    let key1 = FairOrderingPolicy::compute_ordering_key(policy, &timing1, &hash1, 1000);
    let key2 = FairOrderingPolicy::compute_ordering_key(policy, &timing2, &hash2, 2000);

    // Earlier sequence number should come first
    assert!(key1 < key2);
}

#[test]
fn test_batched_ordering() {
    let policy = FairOrderingPolicy::BatchedRandom {
        batch_interval_ms: 100,
    };

    let timing1 = TransactionTiming::new(1).with_batch(1);
    let timing2 = TransactionTiming::new(2).with_batch(1);
    let timing3 = TransactionTiming::new(3).with_batch(2);

    let hash1 = [0u8; 32];
    let hash2 = [1u8; 32];
    let hash3 = [2u8; 32];

    let key1 = FairOrderingPolicy::compute_ordering_key(policy, &timing1, &hash1, 1000);
    let key2 = FairOrderingPolicy::compute_ordering_key(policy, &timing2, &hash2, 1000);
    let key3 = FairOrderingPolicy::compute_ordering_key(policy, &timing3, &hash3, 1000);

    // Same batch should have same primary key
    assert_eq!(key1.primary, key2.primary);
    // Different batch should have different primary key
    assert!(key1.primary < key3.primary);
}
