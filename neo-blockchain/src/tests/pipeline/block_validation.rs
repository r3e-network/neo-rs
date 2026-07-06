use super::*;
use neo_primitives::constants::{MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use neo_primitives::{TimeProvider, UInt256};

#[test]
fn block_validation_error_display_messages_remain_stable() {
    let hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
    assert_eq!(
        BlockValidationError::BlockTooLarge {
            size: 11,
            max_size: 10
        }
        .to_string(),
        "Block size 11 exceeds maximum 10"
    );
    assert_eq!(
        BlockValidationError::TooManyTransactions {
            count: 12,
            max_count: 11
        }
        .to_string(),
        "Transaction count 12 exceeds maximum 11"
    );
    assert_eq!(
        BlockValidationError::TimestampTooFarInFuture {
            timestamp: 20,
            current: 10
        }
        .to_string(),
        "Timestamp 20 is too far in future (current: 10)"
    );
    assert_eq!(
        BlockValidationError::TimestampTooOld {
            timestamp: 1,
            min: 2
        }
        .to_string(),
        "Timestamp 1 is before minimum 2"
    );
    assert_eq!(
        BlockValidationError::TimestampNotIncreasing {
            timestamp: 7,
            prev_timestamp: 8
        }
        .to_string(),
        "Timestamp 7 must be greater than previous 8"
    );
    assert_eq!(
        BlockValidationError::InvalidMerkleRoot {
            expected: hash,
            computed: UInt256::default()
        }
        .to_string(),
        format!(
            "Merkle root mismatch: expected {}, computed {}",
            hash,
            UInt256::default()
        )
    );
    assert_eq!(
        BlockValidationError::DuplicateTransactions.to_string(),
        "Block contains duplicate transactions"
    );
    assert_eq!(
        BlockValidationError::TransactionVerificationFailed { index: 3, hash }.to_string(),
        format!("Transaction {} at index 3 failed verification", hash)
    );
    assert_eq!(
        BlockValidationError::InvalidWitnessScript {
            reason: "bad opcode".to_string()
        }
        .to_string(),
        "Invalid witness script: bad opcode"
    );
    assert_eq!(
        BlockValidationError::EmptyTransactionList.to_string(),
        "Block has empty transaction list"
    );
    assert_eq!(
        BlockValidationError::UnsupportedVersion { version: 2 }.to_string(),
        "Block version 2 is not supported"
    );
    assert_eq!(
        BlockValidationError::InvalidPrimaryIndex { index: 8, max: 7 }.to_string(),
        "Primary index 8 exceeds maximum validator count 7"
    );
    assert_eq!(
        BlockValidationError::HeaderValidationFailed {
            reason: "bad header".to_string()
        }
        .to_string(),
        "Header validation failed: bad header"
    );
}

#[test]
fn validate_block_version_accepts_version_0() {
    assert!(BlockValidator::validate_block_version(0).is_ok());
}

#[test]
fn validate_block_version_rejects_unsupported_versions() {
    assert_eq!(
        BlockValidator::validate_block_version(1),
        Err(BlockValidationError::UnsupportedVersion { version: 1 })
    );
    assert_eq!(
        BlockValidator::validate_block_version(99),
        Err(BlockValidationError::UnsupportedVersion { version: 99 })
    );
}

#[test]
fn validate_block_size_raw_accepts_valid_size() {
    assert!(BlockValidator::validate_block_size_raw(1000).is_ok());
    assert!(BlockValidator::validate_block_size_raw(MAX_BLOCK_SIZE).is_ok());
}

#[test]
fn validate_block_size_raw_rejects_oversized() {
    assert_eq!(
        BlockValidator::validate_block_size_raw(MAX_BLOCK_SIZE + 1),
        Err(BlockValidationError::BlockTooLarge {
            size: MAX_BLOCK_SIZE + 1,
            max_size: MAX_BLOCK_SIZE,
        })
    );
}

#[test]
fn validate_transaction_count_raw_accepts_valid_count() {
    assert!(BlockValidator::validate_transaction_count_raw(100).is_ok());
    assert!(BlockValidator::validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK).is_ok());
}

#[test]
fn validate_transaction_count_raw_rejects_too_many() {
    assert_eq!(
        BlockValidator::validate_transaction_count_raw(MAX_TRANSACTIONS_PER_BLOCK + 1),
        Err(BlockValidationError::TooManyTransactions {
            count: MAX_TRANSACTIONS_PER_BLOCK + 1,
            max_count: MAX_TRANSACTIONS_PER_BLOCK,
        })
    );
}

#[test]
fn validate_transaction_count_raw_with_limit_uses_effective_protocol_limit() {
    assert!(BlockValidator::validate_transaction_count_raw_with_limit(200, 200).is_ok());
    assert_eq!(
        BlockValidator::validate_transaction_count_raw_with_limit(201, 200),
        Err(BlockValidationError::TooManyTransactions {
            count: 201,
            max_count: 200,
        })
    );
}

#[test]
fn validate_timestamp_bounds_accepts_valid_timestamp() {
    let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
    let valid_timestamp = current_time;
    assert!(BlockValidator::validate_timestamp_bounds(valid_timestamp).is_ok());
}

#[test]
fn validate_timestamp_bounds_rejects_past_timestamp() {
    let past_timestamp = MIN_TIMESTAMP_MS - 1;
    assert_eq!(
        BlockValidator::validate_timestamp_bounds(past_timestamp),
        Err(BlockValidationError::TimestampTooOld {
            timestamp: past_timestamp,
            min: MIN_TIMESTAMP_MS,
        })
    );
}

#[test]
fn validate_timestamp_bounds_rejects_far_future() {
    let current_time = TimeProvider::current().utc_now_timestamp_millis() as u64;
    let future_timestamp = current_time + MAX_TIMESTAMP_DRIFT_MS + 10_000;
    let result = BlockValidator::validate_timestamp_bounds(future_timestamp);
    assert!(matches!(
        result,
        Err(BlockValidationError::TimestampTooFarInFuture { .. })
    ));
}

#[test]
fn validate_timestamp_progression_accepts_increasing() {
    assert!(BlockValidator::validate_timestamp_progression(2000, 1000).is_ok());
    assert!(BlockValidator::validate_timestamp_progression(1001, 1000).is_ok());
}

#[test]
fn validate_timestamp_progression_rejects_non_increasing() {
    assert_eq!(
        BlockValidator::validate_timestamp_progression(1000, 1000),
        Err(BlockValidationError::TimestampNotIncreasing {
            timestamp: 1000,
            prev_timestamp: 1000,
        })
    );
    assert_eq!(
        BlockValidator::validate_timestamp_progression(999, 1000),
        Err(BlockValidationError::TimestampNotIncreasing {
            timestamp: 999,
            prev_timestamp: 1000,
        })
    );
}

#[test]
fn validate_merkle_root_accepts_empty_block() {
    let merkle_root = UInt256::default();
    let tx_hashes: Vec<UInt256> = vec![];
    assert!(BlockValidator::validate_merkle_root(&merkle_root, &tx_hashes).is_ok());
}

#[test]
fn validate_merkle_root_rejects_wrong_root_for_empty() {
    let wrong_root = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let tx_hashes: Vec<UInt256> = vec![];
    assert!(BlockValidator::validate_merkle_root(&wrong_root, &tx_hashes).is_err());
}

#[test]
fn validate_import_integrity_rejects_wrong_empty_block_merkle_root() {
    let mut header = neo_payloads::Header::new();
    header.set_merkle_root(UInt256::from([0x42; 32]));
    let block = neo_payloads::Block::from_parts(header, Vec::new());

    assert!(matches!(
        BlockValidator::validate_import_integrity(&block),
        Err(BlockValidationError::InvalidMerkleRoot { .. })
    ));
}

#[test]
fn validate_import_integrity_does_not_enforce_production_tx_limit() {
    let mut block = neo_payloads::Block::new();
    block.transactions = (0..=MAX_TRANSACTIONS_PER_BLOCK)
        .map(|i| {
            let mut tx = neo_payloads::Transaction::new();
            tx.set_nonce(i as u32 + 1);
            tx.set_script(vec![(i % 251) as u8, (i / 251) as u8]);
            tx
        })
        .collect();
    block
        .try_rebuild_merkle_root()
        .expect("valid transaction merkle root");

    assert!(
        BlockValidator::validate_import_integrity(&block).is_ok(),
        "Neo C# treats MaxTransactionsPerBlock as a production limit, not a peer block validity rule"
    );
}

#[test]
fn validate_no_duplicate_transactions_accepts_unique() {
    let hash_a = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let hash_b = UInt256::from_bytes(&[2u8; 32]).unwrap();
    let tx_hashes = vec![hash_a, hash_b];
    assert!(BlockValidator::validate_no_duplicate_transactions(&tx_hashes).is_ok());
}

#[test]
fn validate_no_duplicate_transactions_rejects_duplicates() {
    let hash_a = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let tx_hashes = vec![hash_a, hash_a];
    assert!(BlockValidator::validate_no_duplicate_transactions(&tx_hashes).is_err());
}

#[test]
fn validate_primary_index_accepts_valid() {
    assert!(BlockValidator::validate_primary_index(0, 7).is_ok());
    assert!(BlockValidator::validate_primary_index(6, 7).is_ok());
}

#[test]
fn validate_primary_index_rejects_invalid() {
    assert_eq!(
        BlockValidator::validate_primary_index(7, 7),
        Err(BlockValidationError::InvalidPrimaryIndex { index: 7, max: 7 })
    );
    assert_eq!(
        BlockValidator::validate_primary_index(10, 7),
        Err(BlockValidationError::InvalidPrimaryIndex { index: 10, max: 7 })
    );
}

#[test]
fn validate_witness_scripts_accepts_valid() {
    let witness = Witness::new();
    assert!(BlockValidator::validate_witness_scripts(&witness).is_ok());
}

#[test]
fn validate_witness_scripts_rejects_oversized_invocation() {
    let witness = Witness::new_with_scripts(vec![0u8; 1025], vec![]);
    assert!(BlockValidator::validate_witness_scripts(&witness).is_err());
}

#[test]
fn validate_witness_scripts_rejects_oversized_verification() {
    let witness = Witness::new_with_scripts(vec![], vec![0u8; 1025]);
    assert!(BlockValidator::validate_witness_scripts(&witness).is_err());
}

#[test]
fn max_constants_are_correct() {
    assert_eq!(MAX_BLOCK_SIZE, 2_097_152);
    assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512);
    assert_eq!(MAX_TIMESTAMP_DRIFT_MS, 900_000);
    assert_eq!(MIN_TIMESTAMP_MS, 1468595301000);
}
