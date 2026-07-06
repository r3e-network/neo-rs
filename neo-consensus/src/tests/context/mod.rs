//! # neo-consensus::tests::context
//!
//! Test module grouping Runtime context records carried through the local
//! workflow. coverage for neo-consensus.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-consensus; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use crate::ConsensusError;

fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
    (0..count)
        .map(|i| ValidatorInfo {
            index: i as u8,
            public_key: ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
            script_hash: UInt160::zero(),
        })
        .collect()
}

fn message_hash(index: u32) -> UInt256 {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..4].copy_from_slice(&index.to_le_bytes());
    UInt256::from_bytes(&hash_bytes).unwrap()
}

#[tokio::test]
async fn test_consensus_context_new() {
    let validators = create_test_validators(7);
    let ctx = ConsensusContext::new(100, validators, Some(0), None);

    assert_eq!(ctx.block_index, 100);
    assert_eq!(ctx.view_number, 0);
    assert_eq!(ctx.validator_count(), 7);
    assert_eq!(ctx.my_index, Some(0));
}

#[tokio::test]
async fn test_f_and_m_calculations() {
    // 7 validators: f = 2, M = 5
    let validators = create_test_validators(7);
    let ctx = ConsensusContext::new(0, validators, None, None);
    assert_eq!(ctx.f(), 2);
    assert_eq!(ctx.m(), 5);

    // 4 validators: f = 1, M = 3
    let validators = create_test_validators(4);
    let ctx = ConsensusContext::new(0, validators, None, None);
    assert_eq!(ctx.f(), 1);
    assert_eq!(ctx.m(), 3);

    // 21 validators: f = 6, M = 15
    let validators = create_test_validators(21);
    let ctx = ConsensusContext::new(0, validators, None, None);
    assert_eq!(ctx.f(), 6);
    assert_eq!(ctx.m(), 15);
}

#[tokio::test]
async fn test_primary_index() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(0, validators, Some(0), None);

    // Block 0, view 0: primary = 0
    assert_eq!(ctx.primary_index(), 0);
    assert!(ctx.is_primary());

    // Block 0, view 1: primary = (0 - 1) mod 7 = 6 (matches C# DBFTPlugin)
    ctx.view_number = 1;
    assert_eq!(ctx.primary_index(), 6);
    assert!(!ctx.is_primary());

    // Block 7, view 0: primary = 0 (7 % 7 = 0)
    ctx.block_index = 7;
    ctx.view_number = 0;
    assert_eq!(ctx.primary_index(), 0);
}

#[tokio::test]
async fn test_has_enough_responses() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(0, validators, Some(0), None);

    // Need M = 5 responses
    assert!(!ctx.has_enough_prepare_responses());

    ctx.prepare_request_received = true;
    ctx.prepare_responses.insert(1, vec![1]);
    ctx.prepare_responses.insert(2, vec![2]);
    ctx.prepare_responses.insert(3, vec![3]);
    assert!(!ctx.has_enough_prepare_responses()); // 4 < 5

    ctx.prepare_responses.insert(4, vec![4]);
    assert!(ctx.has_enough_prepare_responses()); // 5 >= 5
}

#[tokio::test]
async fn test_missing_proposed_transaction_tracking() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(0, validators, Some(1), Some(1_000));
    let first = UInt256::from([0x01; 32]);
    let second = UInt256::from([0x02; 32]);
    let unrelated = UInt256::from([0x99; 32]);

    ctx.proposed_tx_hashes = vec![first, second];
    assert!(ctx.has_missing_proposed_transactions());

    ctx.mark_available_transactions([first, unrelated]);
    assert!(ctx.has_missing_proposed_transactions());

    ctx.mark_available_transactions([first, second]);
    assert!(!ctx.has_missing_proposed_transactions());
}

#[tokio::test]
async fn test_reset_for_new_view() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(0, validators, Some(1), None);

    ctx.prepare_request_received = true;
    ctx.prepare_responses.insert(0, vec![0]);
    ctx.commits.insert(0, vec![0]);
    ctx.commit_view_numbers.insert(0, 0);

    ctx.reset_for_new_view(1, 1000);

    assert_eq!(ctx.view_number, 1);
    assert_eq!(ctx.view_start_time, 1000);
    assert!(!ctx.prepare_request_received);
    assert!(ctx.prepare_responses.is_empty());
    assert!(!ctx.commits.is_empty());
}

#[tokio::test]
async fn test_reset_for_new_block_initializes_last_seen_messages() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(0, validators, Some(2), None);

    ctx.reset_for_new_block(10, 1_000);

    assert_eq!(ctx.last_seen_messages.get(&0), Some(&9));
    assert_eq!(ctx.last_seen_messages.get(&1), Some(&9));
    assert_eq!(ctx.last_seen_messages.get(&2), Some(&10));
    assert_eq!(ctx.last_seen_messages.get(&3), Some(&9));
    assert_eq!(ctx.count_failed(), 0);
}

#[tokio::test]
async fn test_reset_for_new_block_preserves_last_seen_messages() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(0, validators, Some(2), None);

    ctx.reset_for_new_block(10, 1_000);
    ctx.last_seen_messages.insert(0, 10);
    ctx.last_seen_messages.insert(1, 7);

    ctx.reset_for_new_block(11, 2_000);

    assert_eq!(ctx.last_seen_messages.get(&0), Some(&10));
    assert_eq!(ctx.last_seen_messages.get(&1), Some(&7));
    assert_eq!(ctx.last_seen_messages.get(&2), Some(&11));
    assert_eq!(ctx.last_seen_messages.get(&3), Some(&9));
    assert_eq!(ctx.count_failed(), 2);
}

#[tokio::test]
async fn test_timeout_calculation() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(0, validators, None, None);

    // View 0: base << 1 = 30s (matches C# shift by ViewNumber+1)
    assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 2);
    assert_eq!(ctx.prepare_request_delay(), BLOCK_TIME_MS);
    assert_eq!(ctx.prepare_request_follow_up_delay(), BLOCK_TIME_MS);
    assert_eq!(ctx.primary_timeout_delay(), BLOCK_TIME_MS * 2);
    assert_eq!(ctx.commit_recovery_resend_delay(), BLOCK_TIME_MS * 2);
    assert_eq!(ctx.change_view_retry_delay(), BLOCK_TIME_MS * 4);

    ctx.view_start_time = 1_000;
    assert!(!ctx.is_timed_out(1_000 + BLOCK_TIME_MS * 2 - 1));
    assert!(ctx.is_timed_out(1_000 + BLOCK_TIME_MS * 2));

    // View 1: base << 2 = 60s
    ctx.view_number = 1;
    assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 4);
    assert_eq!(ctx.prepare_request_delay(), BLOCK_TIME_MS);
    assert_eq!(ctx.prepare_request_follow_up_delay(), BLOCK_TIME_MS * 4);
    assert_eq!(ctx.primary_timeout_delay(), BLOCK_TIME_MS * 5);

    // View 2: base << 3 = 120s
    ctx.view_number = 2;
    assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 8);

    // View 10: base << 11 = 30720s (C# uses 32-bit shift with 5-bit mask)
    // In C#, `TimePerBlock << (ViewNumber + 1)` masks the shift amount to 5 bits,
    // so the timeout grows exponentially until view 30, then wraps at view 31.
    ctx.view_number = 10;
    assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 2048);
}

#[tokio::test]
async fn extend_timer_by_factor_pushes_the_change_view_deadline_later() {
    let validators = create_test_validators(7);
    // my_index = Some(0): a validator (not watch-only), M = 7 - f(2) = 5.
    let mut ctx = ConsensusContext::new(0, validators, Some(0), None);
    let m = ctx.m() as u64;
    assert_eq!(m, 5);
    ctx.view_start_time = 1_000;
    let base_deadline = 1_000 + ctx.get_timeout();
    assert!(ctx.is_timed_out(base_deadline));
    assert!(!ctx.is_timed_out(base_deadline - 1));

    // C# ExtendTimerByFactor(2): deadline moves later by 2 * base_block_time / M.
    ctx.extend_timer_by_factor(2);
    let ext2 = 2 * BLOCK_TIME_MS / m;
    assert_eq!(ctx.timer_extension, ext2);
    assert!(!ctx.is_timed_out(base_deadline));
    assert!(ctx.is_timed_out(base_deadline + ext2));

    // Never decreases: a smaller factor leaves it unchanged; a larger one grows it.
    ctx.extend_timer_by_factor(1);
    assert_eq!(ctx.timer_extension, ext2);
    ctx.extend_timer_by_factor(4);
    assert_eq!(ctx.timer_extension, 4 * BLOCK_TIME_MS / m);

    // A watch-only node (no my_index) never extends.
    let mut watcher = ConsensusContext::new(0, create_test_validators(7), None, None);
    watcher.extend_timer_by_factor(4);
    assert_eq!(watcher.timer_extension, 0);

    // Reset on a new view.
    ctx.reset_for_new_view(1, 2_000);
    assert_eq!(ctx.timer_extension, 0);
}

#[tokio::test]
async fn test_save_and_load_roundtrip() {
    use std::env;

    // Create a test context with some state
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators.clone(), None, Some(0));

    // Set up some consensus state
    ctx.view_number = 2;
    ctx.proposed_block_hash = Some(UInt256::from_bytes(&[1u8; 32]).unwrap());
    ctx.preparation_hash = Some(UInt256::from_bytes(&[9u8; 32]).unwrap());
    ctx.proposed_timestamp = 1234567890;
    ctx.proposed_tx_hashes = vec![
        UInt256::from_bytes(&[2u8; 32]).unwrap(),
        UInt256::from_bytes(&[3u8; 32]).unwrap(),
    ];
    ctx.nonce = 42;
    ctx.prepare_request_received = true;
    ctx.prepare_request_invocation = Some(vec![0x0c, 0x40, 0xaa]);
    ctx.prepare_responses.insert(1, vec![0xaa, 0xbb, 0xcc]);
    ctx.prepare_responses.insert(2, vec![0xdd, 0xee, 0xff]);
    ctx.prepare_response_hashes
        .insert(1, UInt256::from_bytes(&[0x10; 32]).unwrap());
    ctx.prepare_response_hashes
        .insert(2, UInt256::from_bytes(&[0x11; 32]).unwrap());
    ctx.commits.insert(0, vec![0x11, 0x22, 0x33]);
    ctx.commits.insert(1, vec![0x44, 0x55, 0x66]);
    ctx.commit_view_numbers.insert(0, 2);
    ctx.commit_view_numbers.insert(1, 2);
    ctx.commit_invocations.insert(0, vec![0x0c, 0x40, 0xbb]);
    ctx.commit_invocations.insert(1, vec![0x0c, 0x40, 0xcc]);
    ctx.change_views.insert(3, (3, ChangeViewReason::Timeout));
    ctx.change_views
        .insert(4, (3, ChangeViewReason::TxNotFound));
    ctx.change_view_invocations
        .insert(3, vec![0x0c, 0x40, 0xdd]);
    ctx.change_view_invocations
        .insert(4, vec![0x0c, 0x40, 0xee]);
    ctx.last_change_view_timestamps.insert(3, 1_111);
    ctx.last_change_view_timestamps.insert(4, 2_222);

    // Save to a temporary file
    let temp_dir = env::temp_dir();
    let temp_path = temp_dir.join("test_consensus_state.bin");

    ctx.save(&temp_path).expect("Failed to save context");

    // Load it back
    let loaded_ctx =
        ConsensusContext::load(&temp_path, validators, Some(0)).expect("Failed to load context");

    // Verify all persisted fields match
    assert_eq!(loaded_ctx.block_index, 100);
    assert_eq!(loaded_ctx.view_number, 2);
    assert_eq!(
        loaded_ctx.proposed_block_hash,
        Some(UInt256::from_bytes(&[1u8; 32]).unwrap())
    );
    assert_eq!(
        loaded_ctx.preparation_hash,
        Some(UInt256::from_bytes(&[9u8; 32]).unwrap())
    );
    assert_eq!(loaded_ctx.proposed_timestamp, 1234567890);
    assert_eq!(loaded_ctx.proposed_tx_hashes.len(), 2);
    assert_eq!(
        loaded_ctx.proposed_tx_hashes[0],
        UInt256::from_bytes(&[2u8; 32]).unwrap()
    );
    assert_eq!(
        loaded_ctx.proposed_tx_hashes[1],
        UInt256::from_bytes(&[3u8; 32]).unwrap()
    );
    assert_eq!(loaded_ctx.nonce, 42);
    assert!(loaded_ctx.prepare_request_received);
    assert_eq!(
        loaded_ctx.prepare_request_invocation,
        Some(vec![0x0c, 0x40, 0xaa])
    );
    assert_eq!(loaded_ctx.prepare_responses.len(), 2);
    assert_eq!(
        loaded_ctx.prepare_responses.get(&1),
        Some(&vec![0xaa, 0xbb, 0xcc])
    );
    assert_eq!(
        loaded_ctx.prepare_responses.get(&2),
        Some(&vec![0xdd, 0xee, 0xff])
    );
    assert_eq!(loaded_ctx.prepare_response_hashes.len(), 2);
    assert_eq!(
        loaded_ctx.prepare_response_hashes.get(&1),
        Some(&UInt256::from_bytes(&[0x10; 32]).unwrap())
    );
    assert_eq!(
        loaded_ctx.prepare_response_hashes.get(&2),
        Some(&UInt256::from_bytes(&[0x11; 32]).unwrap())
    );
    assert_eq!(loaded_ctx.commits.len(), 2);
    assert_eq!(loaded_ctx.commits.get(&0), Some(&vec![0x11, 0x22, 0x33]));
    assert_eq!(loaded_ctx.commits.get(&1), Some(&vec![0x44, 0x55, 0x66]));
    assert_eq!(loaded_ctx.commit_view_numbers.get(&0), Some(&2));
    assert_eq!(loaded_ctx.commit_view_numbers.get(&1), Some(&2));
    assert_eq!(
        loaded_ctx.commit_invocations.get(&0),
        Some(&vec![0x0c, 0x40, 0xbb])
    );
    assert_eq!(
        loaded_ctx.commit_invocations.get(&1),
        Some(&vec![0x0c, 0x40, 0xcc])
    );
    assert_eq!(loaded_ctx.change_views.len(), 2);
    assert_eq!(
        loaded_ctx.change_views.get(&3),
        Some(&(3, ChangeViewReason::Timeout))
    );
    assert_eq!(
        loaded_ctx.change_views.get(&4),
        Some(&(3, ChangeViewReason::TxNotFound))
    );
    assert_eq!(
        loaded_ctx.change_view_invocations.get(&3),
        Some(&vec![0x0c, 0x40, 0xdd])
    );
    assert_eq!(
        loaded_ctx.change_view_invocations.get(&4),
        Some(&vec![0x0c, 0x40, 0xee])
    );
    assert_eq!(loaded_ctx.last_change_view_timestamps.get(&3), Some(&1_111));
    assert_eq!(loaded_ctx.last_change_view_timestamps.get(&4), Some(&2_222));

    // Verify non-persisted fields are reset
    assert_eq!(loaded_ctx.state, ConsensusState::Initial);
    assert_eq!(loaded_ctx.view_start_time, 0);
    assert_eq!(loaded_ctx.expected_block_time, 0);
    assert!(loaded_ctx.last_seen_messages.is_empty());

    // Clean up
    let _ = std::fs::remove_file(&temp_path);
}

#[tokio::test]
async fn test_save_empty_state() {
    use std::env;

    // Create a minimal context
    let validators = create_test_validators(4);
    let ctx = ConsensusContext::new(0, validators.clone(), None, None);

    // Save to a temporary file
    let temp_dir = env::temp_dir();
    let temp_path = temp_dir.join("test_consensus_empty.bin");

    ctx.save(&temp_path).expect("Failed to save empty context");

    // Load it back
    let loaded_ctx =
        ConsensusContext::load(&temp_path, validators, None).expect("Failed to load empty context");

    // Verify basic fields
    assert_eq!(loaded_ctx.block_index, 0);
    assert_eq!(loaded_ctx.view_number, 0);
    assert_eq!(loaded_ctx.proposed_block_hash, None);
    assert!(!loaded_ctx.prepare_request_received);
    assert!(loaded_ctx.prepare_responses.is_empty());
    assert!(loaded_ctx.prepare_response_hashes.is_empty());
    assert!(loaded_ctx.commits.is_empty());
    assert!(loaded_ctx.change_views.is_empty());
    assert!(loaded_ctx.change_view_invocations.is_empty());
    assert!(loaded_ctx.commit_invocations.is_empty());
    assert!(loaded_ctx.prepare_request_invocation.is_none());
    assert!(loaded_ctx.commit_view_numbers.is_empty());

    // Clean up
    let _ = std::fs::remove_file(&temp_path);
}

#[tokio::test]
async fn test_save_atomic_write() {
    use std::env;

    let validators = create_test_validators(4);
    let ctx = ConsensusContext::new(42, validators, Some(1), None);

    let temp_dir = env::temp_dir();
    let temp_path = temp_dir.join("test_consensus_atomic.bin");

    // Save should succeed
    ctx.save(&temp_path).expect("Failed to save");

    // Verify the temp file is cleaned up
    let temp_tmp_path = temp_path.with_extension("tmp");
    assert!(!temp_tmp_path.exists(), "Temp file should be cleaned up");

    // Verify the final file exists
    assert!(temp_path.exists(), "Final file should exist");

    // Clean up
    let _ = std::fs::remove_file(&temp_path);
}

#[tokio::test]
async fn test_load_nonexistent_file() {
    use std::env;

    let validators = create_test_validators(4);
    let temp_dir = env::temp_dir();
    let nonexistent_path = temp_dir.join("nonexistent_consensus_state.bin");

    // Should return an IO error
    let result = ConsensusContext::load(&nonexistent_path, validators, None);
    assert!(result.is_err());
    match result {
        Err(ConsensusError::IoError(_)) => {} // Expected
        _ => panic!("Expected IoError"),
    }
}

#[tokio::test]
async fn test_load_corrupted_file() {
    use std::env;

    let validators = create_test_validators(4);
    let temp_dir = env::temp_dir();
    let corrupted_path = temp_dir.join("test_consensus_corrupted.bin");

    // Write garbage data
    std::fs::write(&corrupted_path, b"this is not valid bincode data")
        .expect("Failed to write corrupted file");

    // Should return a serialization error
    let result = ConsensusContext::load(&corrupted_path, validators, None);
    assert!(result.is_err());
    match result {
        Err(ConsensusError::BincodeError(_)) => {} // Expected
        _ => panic!("Expected BincodeError"),
    }

    // Clean up
    let _ = std::fs::remove_file(&corrupted_path);
}

#[tokio::test]
async fn test_count_committed() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Initially no commits
    assert_eq!(ctx.count_committed(), 0);

    // Add some commits
    ctx.commits.insert(0, vec![0x11]);
    assert_eq!(ctx.count_committed(), 1);

    ctx.commits.insert(1, vec![0x22]);
    ctx.commits.insert(2, vec![0x33]);
    assert_eq!(ctx.count_committed(), 3);
}

#[tokio::test]
async fn test_count_failed_empty() {
    let validators = create_test_validators(7);
    let ctx = ConsensusContext::new(100, validators, Some(0), None);

    // No last_seen_messages tracked yet
    assert_eq!(ctx.count_failed(), 0);
}

#[tokio::test]
async fn test_count_failed_with_old_messages() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Simulate messages from validators at different block heights
    ctx.last_seen_messages.insert(0, 100); // Current block - not failed
    ctx.last_seen_messages.insert(1, 99); // Previous block - not failed
    ctx.last_seen_messages.insert(2, 98); // Old block (< 99) - FAILED
    ctx.last_seen_messages.insert(3, 95); // Very old block - FAILED
    // Validators 4, 5, 6 have no messages - FAILED

    // Failed: validators 2, 3, 4, 5, 6 = 5 validators
    assert_eq!(ctx.count_failed(), 5);
}

#[tokio::test]
async fn test_count_failed_threshold() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(10, validators, Some(0), None);

    // Block 10, threshold is 9 (block_index - 1)
    // Messages at block 9 or higher are OK
    // Messages at block 8 or lower are failed

    ctx.last_seen_messages.insert(0, 10); // OK
    ctx.last_seen_messages.insert(1, 9); // OK (exactly at threshold)
    ctx.last_seen_messages.insert(2, 8); // FAILED (< threshold)
    // Validator 3 has no message - FAILED

    assert_eq!(ctx.count_failed(), 2); // Validators 2 and 3
}

#[tokio::test]
async fn test_more_than_f_nodes_committed_or_lost() {
    // 7 validators: f = 2, M = 5
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Initially: committed=0, failed=0, total=0, f=2
    // 0 > 2? No
    assert!(!ctx.more_than_f_nodes_committed_or_lost());

    // Add 2 commits: committed=2, failed=0, total=2, f=2
    // 2 > 2? No
    ctx.commits.insert(0, vec![0x11]);
    ctx.commits.insert(1, vec![0x22]);
    assert!(!ctx.more_than_f_nodes_committed_or_lost());

    // Add 1 more commit: committed=3, failed=0, total=3, f=2
    // 3 > 2? Yes - SHOULD REQUEST RECOVERY
    ctx.commits.insert(2, vec![0x33]);
    assert!(ctx.more_than_f_nodes_committed_or_lost());
}

#[tokio::test]
async fn test_not_accepting_payloads_due_to_view_changing() {
    // 4 validators: f = 1, M = 3
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(1), None);

    assert!(!ctx.view_changing());
    assert!(!ctx.not_accepting_payloads_due_to_view_changing());

    // Simulate requesting a view change.
    ctx.change_views.insert(1, (1, ChangeViewReason::Timeout));
    assert!(ctx.view_changing());
    assert!(ctx.not_accepting_payloads_due_to_view_changing());

    // If more than F nodes committed or are lost, accept payloads again.
    ctx.commits.insert(0, vec![0x11]);
    ctx.commits.insert(2, vec![0x22]);
    assert!(ctx.more_than_f_nodes_committed_or_lost());
    assert!(!ctx.not_accepting_payloads_due_to_view_changing());
}

#[tokio::test]
async fn test_more_than_f_nodes_with_failed() {
    // 7 validators: f = 2, M = 5
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Simulate 2 commits and 1 failed node
    ctx.commits.insert(0, vec![0x11]);
    ctx.commits.insert(1, vec![0x22]);

    ctx.last_seen_messages.insert(0, 100);
    ctx.last_seen_messages.insert(1, 100);
    ctx.last_seen_messages.insert(2, 100);
    ctx.last_seen_messages.insert(3, 100);
    ctx.last_seen_messages.insert(4, 100);
    ctx.last_seen_messages.insert(5, 100);
    ctx.last_seen_messages.insert(6, 95); // Old message - FAILED

    // committed=2, failed=1, total=3, f=2
    // 3 > 2? Yes - SHOULD REQUEST RECOVERY
    assert_eq!(ctx.count_committed(), 2);
    assert_eq!(ctx.count_failed(), 1);
    assert!(ctx.more_than_f_nodes_committed_or_lost());
}

#[tokio::test]
async fn test_more_than_f_nodes_edge_case() {
    // 4 validators: f = 1, M = 3
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(50, validators, Some(0), None);

    // committed=1, failed=0, total=1, f=1
    // 1 > 1? No
    ctx.commits.insert(0, vec![0x11]);
    assert!(!ctx.more_than_f_nodes_committed_or_lost());

    // committed=1, failed=1, total=2, f=1
    // 2 > 1? Yes - SHOULD REQUEST RECOVERY
    ctx.last_seen_messages.insert(0, 50);
    ctx.last_seen_messages.insert(1, 50);
    ctx.last_seen_messages.insert(2, 50);
    ctx.last_seen_messages.insert(3, 45); // Old - FAILED

    assert_eq!(ctx.count_failed(), 1);
    assert!(ctx.more_than_f_nodes_committed_or_lost());
}

#[tokio::test]
async fn test_update_last_seen_message() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    assert!(ctx.last_seen_messages.is_empty());

    ctx.update_last_seen_message(0, 100);
    assert_eq!(ctx.last_seen_messages.get(&0), Some(&100));

    ctx.update_last_seen_message(1, 101);
    assert_eq!(ctx.last_seen_messages.get(&1), Some(&101));

    // Update existing entry
    ctx.update_last_seen_message(0, 102);
    assert_eq!(ctx.last_seen_messages.get(&0), Some(&102));
}

#[tokio::test]
async fn test_message_hash_caching() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Create test message hashes
    let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let hash2 = UInt256::from_bytes(&[2u8; 32]).unwrap();

    // Initially, no messages have been seen
    assert!(!ctx.has_seen_message(&hash1));
    assert!(!ctx.has_seen_message(&hash2));

    // Mark hash1 as seen
    ctx.mark_message_seen(&hash1);
    assert!(ctx.has_seen_message(&hash1));
    assert!(!ctx.has_seen_message(&hash2));

    // Mark hash2 as seen
    ctx.mark_message_seen(&hash2);
    assert!(ctx.has_seen_message(&hash1));
    assert!(ctx.has_seen_message(&hash2));

    // Marking the same hash again should be idempotent
    ctx.mark_message_seen(&hash1);
    assert!(ctx.has_seen_message(&hash1));
}

#[tokio::test]
async fn test_message_cache_cleared_on_new_block() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Add some message hashes
    let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let hash2 = UInt256::from_bytes(&[2u8; 32]).unwrap();
    ctx.mark_message_seen(&hash1);
    ctx.mark_message_seen(&hash2);

    assert!(ctx.has_seen_message(&hash1));
    assert!(ctx.has_seen_message(&hash2));

    // Reset for new block should clear the cache
    ctx.reset_for_new_block(101, 2000);

    assert!(!ctx.has_seen_message(&hash1));
    assert!(!ctx.has_seen_message(&hash2));
    assert_eq!(ctx.block_index, 101);
}

#[tokio::test]
async fn test_message_cache_not_cleared_on_view_change() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Add some message hashes
    let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
    ctx.mark_message_seen(&hash1);
    assert!(ctx.has_seen_message(&hash1));

    // Reset for new view should NOT clear the message cache
    // (messages are still valid within the same block)
    ctx.reset_for_new_view(1, 1000);

    assert!(ctx.has_seen_message(&hash1));
    assert_eq!(ctx.view_number, 1);
}

#[tokio::test]
async fn test_message_cache_prevents_replay() {
    let validators = create_test_validators(7);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    // Simulate receiving the same message twice
    let msg_hash = UInt256::from_bytes(&[0xaa; 32]).unwrap();

    // First time: message is new
    assert!(!ctx.has_seen_message(&msg_hash));
    ctx.mark_message_seen(&msg_hash);

    // Second time: message is duplicate (replay attack)
    assert!(ctx.has_seen_message(&msg_hash));
}

#[tokio::test]
async fn test_message_cache_lru_limit() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    for i in 0..MAX_MESSAGE_CACHE_SIZE {
        ctx.mark_message_seen(&message_hash(i as u32));
    }

    let first_hash = message_hash(0);
    let second_hash = message_hash(1);
    assert!(ctx.has_seen_message(&first_hash));
    assert!(ctx.has_seen_message(&second_hash));
    assert_eq!(ctx.seen_message_hashes.len(), MAX_MESSAGE_CACHE_SIZE);

    let overflow_hash = message_hash(MAX_MESSAGE_CACHE_SIZE as u32);
    ctx.mark_message_seen(&overflow_hash);

    assert!(!ctx.has_seen_message(&first_hash));
    assert!(ctx.has_seen_message(&second_hash));
    assert!(ctx.has_seen_message(&overflow_hash));
    assert_eq!(ctx.seen_message_hashes.len(), MAX_MESSAGE_CACHE_SIZE);
}

#[tokio::test]
async fn test_message_cache_duplicate_and_contains_do_not_refresh_lru_order() {
    let validators = create_test_validators(4);
    let mut ctx = ConsensusContext::new(100, validators, Some(0), None);

    for i in 0..MAX_MESSAGE_CACHE_SIZE {
        ctx.mark_message_seen(&message_hash(i as u32));
    }

    let first_hash = message_hash(0);
    let second_hash = message_hash(1);
    assert!(ctx.has_seen_message(&first_hash));
    ctx.mark_message_seen(&first_hash);

    let overflow_hash = message_hash(MAX_MESSAGE_CACHE_SIZE as u32);
    ctx.mark_message_seen(&overflow_hash);

    assert!(!ctx.has_seen_message(&first_hash));
    assert!(ctx.has_seen_message(&second_hash));
    assert!(ctx.has_seen_message(&overflow_hash));
}

#[tokio::test]
async fn invalid_transactions_skip_set_uses_f_threshold_and_clears_per_block() {
    // 4 validators => F = (4-1)/3 = 1, so a tx must be reported by MORE THAN 1
    // distinct validator (>= 2) before the primary skips it (C# count > F).
    let mut ctx = ConsensusContext::new(10, create_test_validators(4), Some(0), None);
    assert_eq!(ctx.f(), 1);
    let tx = message_hash(0xAB);

    // One reporter: not over F.
    ctx.record_invalid_transactions(0, &[tx]);
    assert!(ctx.invalid_tx_hashes_over_f().is_empty());

    // The same validator re-reporting does not raise the distinct count.
    ctx.record_invalid_transactions(0, &[tx]);
    assert!(ctx.invalid_tx_hashes_over_f().is_empty());

    // A second distinct validator => 2 > F=1 => skipped.
    ctx.record_invalid_transactions(1, &[tx]);
    assert_eq!(ctx.invalid_tx_hashes_over_f(), vec![tx]);

    // Accumulated reports clear on a new block (they persist across views).
    ctx.reset_for_new_block(11, 1_000);
    assert!(ctx.invalid_transactions.is_empty());
    assert!(ctx.invalid_tx_hashes_over_f().is_empty());
}
