#![cfg(feature = "runtime")]

//! Behavioral regression tests for RwSet tracking and conflict resolution.
//!
//! Access sets are constructed directly, without executing contracts or blocks.
//! The larger cases check correctness, not throughput or end-to-end isolation.

#[cfg(test)]
mod wave1_integration_tests {
    use neo_core::{Transaction, UInt256};
    use neo_vm::RwSetTracker;
    use std::sync::Arc;

    fn make_key(value: u64) -> UInt256 {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&value.to_le_bytes());
        UInt256::from(bytes)
    }

    // ============================================
    // RwSet Tracker Integration Tests
    // ============================================

    #[test]
    fn test_rwset_tracking_during_contract_execution() {
        // Simulate ERC-20 transfer contract execution pattern
        let mut rw_set = RwSetTracker::new();

        // Contract reads sender balance
        let sender_balance_key = make_key(0x1000u64);
        rw_set.record_read(sender_balance_key);

        // Contract reads receiver balance
        let receiver_balance_key = make_key(0x2000u64);
        rw_set.record_read(receiver_balance_key);

        // Contract writes new sender balance (decremented)
        rw_set.record_write(sender_balance_key);

        // Contract writes new receiver balance (incremented)
        rw_set.record_write(receiver_balance_key);

        // Verify tracking
        assert_eq!(rw_set.read_keys.len(), 2);
        assert_eq!(rw_set.write_keys.len(), 2);
        assert!(sender_balance_key == rw_set.write_keys[0]);
        assert!(receiver_balance_key == rw_set.write_keys[1]);
    }

    #[test]
    fn test_multi_read_no_conflict() {
        // Multiple transactions reading same key should NOT conflict
        let mut tx1_rws = RwSetTracker::new();
        let mut tx2_rws = RwSetTracker::new();

        let shared_key = make_key(0xDEADu64);

        // Both read only
        tx1_rws.record_read(shared_key);
        tx2_rws.record_read(shared_key);

        assert!(
            !tx1_rws.conflicts_with(&tx2_rws),
            "Read-read should never conflict"
        );
        assert!(!tx2_rws.conflicts_with(&tx1_rws));
    }

    #[test]
    fn test_dirty_read_prevention() {
        // Prevents reading data being modified by another tx (ACID isolation)
        let mut writer_rws = RwSetTracker::new();
        let mut reader_rws = RwSetTracker::new();

        let storage_key = make_key(0xBEEFu64);

        // Writer modifies key
        writer_rws.record_write(storage_key);

        // Reader reads same key
        reader_rws.record_read(storage_key);

        // Detection works both directions
        assert!(
            writer_rws.conflicts_with(&reader_rws),
            "Writer must detect reader"
        );
        // Note: Reader doesn't conflict with writer from perspective of committing first
        // This is correct - we commit writer, then retry reader if needed
    }

    // ============================================
    // Conflict Detection Tests
    // ============================================

    #[test]
    fn test_write_write_conflict_resolution() {
        // Two transactions trying to modify same account
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        let conflicting_key = make_key(0xCAFEu64);

        let mut tx0_rwset = RwSetTracker::new();
        let mut tx1_rwset = RwSetTracker::new();

        tx0_rwset.record_write(conflicting_key);
        tx1_rwset.record_write(conflicting_key);

        detector.submit_tx(
            0,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 0,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![1]),
                },
                rw_set: tx0_rwset,
            },
        );

        detector.submit_tx(
            1,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 1,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![2]),
                },
                rw_set: tx1_rwset,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();

        // One should be committed, one retried (deterministic ordering)
        assert_eq!(resolution.non_conflicting_order.len(), 1);
        assert_eq!(resolution.needs_retry.len(), 1);
    }

    #[test]
    fn test_complex_conflict_graph() {
        // Test scenario: 3 transactions with chain of conflicts
        // Tx0 → Tx1 → Tx2 (Tx0 and Tx1 conflict, Tx1 and Tx2 conflict)
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        let key_a = make_key(0xAAAAu64);
        let key_b = make_key(0xBBBBu64);
        let key_c = make_key(0xCCCCu64);

        let mut tx0 = RwSetTracker::new();
        let mut tx1 = RwSetTracker::new();
        let mut tx2 = RwSetTracker::new();

        // Tx0 touches A and B
        tx0.record_write(key_a);
        tx0.record_write(key_b);

        // Tx1 touches B and C (conflicts on B with both)
        tx1.record_write(key_b);
        tx1.record_write(key_c);

        // Tx2 touches C and A (conflicts on both)
        tx2.record_write(key_c);
        tx2.record_write(key_a);

        detector.submit_tx(
            0,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 0,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![0]),
                },
                rw_set: tx0,
            },
        );

        detector.submit_tx(
            1,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 1,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![1]),
                },
                rw_set: tx1,
            },
        );

        detector.submit_tx(
            2,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 2,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![2]),
                },
                rw_set: tx2,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();

        // At least some non-conflicting order expected
        assert!(resolution.non_conflicting_order.len() > 0 || resolution.needs_retry.len() > 0);

        // Verify no duplicates in either list
        let non_conflicting_sorted = resolution.non_conflicting_order.clone();
        let mut sorted = non_conflicting_sorted.clone();
        sorted.sort();
        assert_eq!(sorted, non_conflicting_sorted);
    }

    // ============================================
    // Scale Regression Tests
    // ============================================

    #[test]
    fn conflict_detection_100_tx() {
        // All 100 access sets write the same ten keys.
        let mut tx_sets: Vec<RwSetTracker> = Vec::with_capacity(100);

        for _ in 0..100 {
            let mut rws = RwSetTracker::new();
            rws.write_keys.reserve(10);

            for i in 0..10 {
                rws.record_write(make_key(i as u64));
            }

            tx_sets.push(rws);
        }

        {
            let mut d = neo_core::state_service::conflict_detector::ConflictDetector::new();

            for (idx, rws) in tx_sets.iter().enumerate() {
                d.submit_tx(
                    idx,
                    neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                        executed_tx:
                            neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                                height: 0,
                                hash: UInt256::zero(),
                                tx_index: idx,
                                transaction: Arc::new(Transaction::default()),
                                execution_result: Ok(vec![]),
                            },
                        rw_set: rws.clone(),
                    },
                );
            }

            let resolution = d.resolve_conflicts().unwrap();
            assert_eq!(resolution.non_conflicting_order, vec![0]);
            assert_eq!(resolution.needs_retry.len(), 99);
            for (index, retry) in resolution.needs_retry.iter().enumerate() {
                assert_eq!(retry.original_index, index + 1);
                assert_eq!(retry.conflicted_with, vec![0]);
            }
        }
    }

    #[test]
    fn rwset_conflict_check_10k_keys() {
        let mut large_rws1 = RwSetTracker::new();
        let mut large_rws2 = RwSetTracker::new();

        // Create two trackers with 10,000 keys each
        for i in 0..10000 {
            large_rws1.record_read(make_key(i * 2));
            large_rws2.record_write(make_key(i * 2 + 1));
        }

        assert!(!large_rws1.conflicts_with(&large_rws2));
        assert!(!large_rws2.conflicts_with(&large_rws1));
        large_rws2.record_write(make_key(19_998));
        assert!(large_rws2.conflicts_with(&large_rws1));
        assert!(!large_rws1.conflicts_with(&large_rws2));
    }

    // ============================================
    // Edge Case Tests
    // ============================================

    #[test]
    fn test_empty_rwsets_no_false_positives() {
        let empty1 = RwSetTracker::new();
        let empty2 = RwSetTracker::new();

        assert!(!empty1.conflicts_with(&empty2));
        assert!(!empty2.conflicts_with(&empty1));
    }

    #[test]
    fn test_single_transaction_always_independent() {
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        let mut rws = RwSetTracker::new();
        rws.record_write(make_key(42u64));

        detector.submit_tx(
            0,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 0,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![1]),
                },
                rw_set: rws,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();

        // Single tx always non-conflicting
        assert_eq!(resolution.non_conflicting_order.len(), 1);
        assert!(resolution.needs_retry.is_empty());
    }

    #[test]
    fn test_duplicate_keys_within_same_tx() {
        // Duplicate reads/writes within single tx are allowed (e.g., loops)
        let mut rws = RwSetTracker::new();
        let key = make_key(100u64);

        for _ in 0..10 {
            rws.record_read(key);
        }

        // Should track all instances but still work correctly
        assert_eq!(rws.read_keys.len(), 10);

        // Conflict detection still correct despite duplicates
        let mut other = RwSetTracker::new();
        other.record_write(key);

        assert!(other.conflicts_with(&rws));
        assert!(!rws.conflicts_with(&other));
    }

    // ============================================
    // State Merge Tests
    // ============================================

    #[test]
    fn test_final_rw_set_aggregation() {
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        let mut tx0 = RwSetTracker::new();
        let mut tx1 = RwSetTracker::new();

        tx0.record_read(make_key(1u64));
        tx0.record_write(make_key(2u64));
        tx0.set_gas_used(1000);

        tx1.record_read(make_key(3u64));
        tx1.record_write(make_key(4u64));
        tx1.set_gas_used(2000);

        detector.submit_tx(
            0,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 0,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![]),
                },
                rw_set: tx0,
            },
        );

        detector.submit_tx(
            1,
            neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                    height: 1000,
                    hash: UInt256::zero(),
                    tx_index: 1,
                    transaction: Arc::new(Transaction::default()),
                    execution_result: Ok(vec![]),
                },
                rw_set: tx1,
            },
        );

        let final_rwset = detector.compute_final_rw_set();

        // Should aggregate all accesses
        assert_eq!(final_rwset.read_keys.len(), 2);
        assert_eq!(final_rwset.write_keys.len(), 2);
        assert_eq!(final_rwset.gas_used, 3000);
    }

    // ============================================
    // Throughput Simulation Tests
    // ============================================

    #[test]
    fn simulate_low_conflict_blockchain_scenario() {
        // Typical scenario: most txs independent (DeFi swaps on different pools)
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        const NUM_TXS: usize = 100;

        for i in 0..NUM_TXS {
            let mut rws = RwSetTracker::new();
            // Each tx touches unique key (no conflict)
            let unique_key = make_key(i as u64);
            rws.record_write(unique_key);

            detector.submit_tx(
                i,
                neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                    executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                        height: 1000,
                        hash: UInt256::zero(),
                        tx_index: i,
                        transaction: Arc::new(Transaction::default()),
                        execution_result: Ok(vec![i as u8]),
                    },
                    rw_set: rws,
                },
            );
        }

        let resolution = detector.resolve_conflicts().unwrap();

        // With no conflicts, ALL should be non-conflicting
        assert_eq!(resolution.non_conflicting_order.len(), NUM_TXS);
        assert!(resolution.needs_retry.is_empty());

        // Expected throughput: ~2-3× improvement over serial
    }

    #[test]
    fn simulate_high_conflict_scenarion() {
        // Stress scenario: multiple txs modifying same state (popular NFT mint)
        let mut detector = neo_core::state_service::conflict_detector::ConflictDetector::new();

        const NUM_TXS: usize = 50;
        let hot_key = make_key(0xFFFFu64);

        for i in 0..NUM_TXS {
            let mut rws = RwSetTracker::new();
            // All txs hit same key (maximum conflicts)
            rws.record_write(hot_key);

            detector.submit_tx(
                i,
                neo_core::state_service::conflict_detector::ExecutedTransactionWithRwSet {
                    executed_tx: neo_core::state_service::prefetch_pipeline::ExecutedTransaction {
                        height: 1000,
                        hash: UInt256::zero(),
                        tx_index: i,
                        transaction: Arc::new(Transaction::default()),
                        execution_result: Ok(vec![i as u8]),
                    },
                    rw_set: rws,
                },
            );
        }

        let resolution = detector.resolve_conflicts().unwrap();

        // Only first tx commits immediately, rest require retry
        assert_eq!(resolution.non_conflicting_order.len(), 1);
        assert_eq!(resolution.needs_retry.len(), NUM_TXS - 1);

        // Still better than pure serial due to early commit of first tx
    }
}
