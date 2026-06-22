use super::*;

#[derive(Debug, Clone)]
struct MockWitness {
    invocation: Vec<u8>,
    verification: Vec<u8>,
}

impl MockWitness {
    fn new(invocation: Vec<u8>, verification: Vec<u8>) -> Self {
        Self {
            invocation,
            verification,
        }
    }
}

impl Witness for MockWitness {
    fn invocation_script(&self) -> &[u8] {
        &self.invocation
    }

    fn verification_script(&self) -> &[u8] {
        &self.verification
    }
}

struct MockVerifier {
    max_gas: i64,
    consumed: i64,
    should_pass: bool,
}

impl MockVerifier {
    fn new(max_gas: i64, should_pass: bool) -> Self {
        Self {
            max_gas,
            consumed: 0,
            should_pass,
        }
    }

    fn with_consumed(mut self, consumed: i64) -> Self {
        self.consumed = consumed;
        self
    }
}

impl VerificationContext for MockVerifier {
    fn verify_witness(&self, _hash: &UInt160, _witness: &dyn Witness) -> VerificationResult<bool> {
        if self.should_pass {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_gas_consumed(&self) -> i64 {
        self.consumed
    }

    fn get_max_gas(&self) -> i64 {
        self.max_gas
    }
}

struct MockSnapshot {
    height: u32,
    storage: std::collections::HashMap<Vec<u8>, Vec<u8>>,
    transactions: std::collections::HashSet<UInt256>,
    blocks: std::collections::HashSet<UInt256>,
}

impl MockSnapshot {
    fn new(height: u32) -> Self {
        Self {
            height,
            storage: std::collections::HashMap::new(),
            transactions: std::collections::HashSet::new(),
            blocks: std::collections::HashSet::new(),
        }
    }

    fn with_storage(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.storage.insert(key, value);
        self
    }

    fn with_transaction(mut self, hash: UInt256) -> Self {
        self.transactions.insert(hash);
        self
    }
}

impl BlockchainSnapshot for MockSnapshot {
    fn height(&self) -> u32 {
        self.height
    }

    fn get_storage(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.storage.get(key).cloned()
    }

    fn contains_transaction(&self, hash: &UInt256) -> bool {
        self.transactions.contains(hash)
    }

    fn contains_block(&self, hash: &UInt256) -> bool {
        self.blocks.contains(hash)
    }

    fn get_block_hash(&self, height: u32) -> Option<UInt256> {
        if height <= self.height {
            Some(UInt256 {
                value1: u64::from(height),
                ..UInt256::zero()
            })
        } else {
            None
        }
    }
}

#[test]
fn test_verification_error_verification_failed() {
    let err = VerificationError::verification_failed("bad signature");
    assert!(err.to_string().contains("witness verification failed"));
    assert!(err.to_string().contains("bad signature"));
}

#[test]
fn test_verification_error_gas_limit_exceeded() {
    let err = VerificationError::gas_limit_exceeded(100, 50);
    assert!(err.to_string().contains("gas limit exceeded"));
    assert!(err.to_string().contains("consumed=100"));
    assert!(err.to_string().contains("max=50"));
}

#[test]
fn test_verification_error_invalid_script() {
    let err = VerificationError::invalid_script("empty script");
    assert!(err.to_string().contains("invalid script"));
    assert!(err.to_string().contains("empty script"));
}

#[test]
fn test_verification_error_invalid_signature() {
    let err = VerificationError::invalid_signature("wrong key");
    assert!(err.to_string().contains("invalid signature"));
    assert!(err.to_string().contains("wrong key"));
}

#[test]
fn test_verification_error_missing_witness() {
    let hash = UInt160::zero();
    let err = VerificationError::missing_witness(&hash);
    assert!(err.to_string().contains("missing witness"));
}

#[test]
fn test_verification_error_clone() {
    let err1 = VerificationError::verification_failed("test");
    let err2 = err1.clone();
    assert_eq!(err1, err2);
}

#[test]
fn test_mock_witness_scripts() {
    let witness = MockWitness::new(vec![0x01, 0x02], vec![0x03, 0x04]);
    assert_eq!(witness.invocation_script(), &[0x01, 0x02]);
    assert_eq!(witness.verification_script(), &[0x03, 0x04]);
}

#[test]
fn test_mock_witness_empty_scripts() {
    let witness = MockWitness::new(vec![], vec![]);
    assert!(witness.invocation_script().is_empty());
    assert!(witness.verification_script().is_empty());
}

#[test]
fn test_mock_verifier_passes() {
    let verifier = MockVerifier::new(1000, true);
    let hash = UInt160::zero();
    let witness = MockWitness::new(vec![0x01], vec![0x02]);

    let result = verifier.verify_witness(&hash, &witness);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_mock_verifier_fails() {
    let verifier = MockVerifier::new(1000, false);
    let hash = UInt160::zero();
    let witness = MockWitness::new(vec![0x01], vec![0x02]);

    let result = verifier.verify_witness(&hash, &witness);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_mock_verifier_gas_tracking() {
    let verifier = MockVerifier::new(1000, true).with_consumed(500);
    assert_eq!(verifier.get_gas_consumed(), 500);
    assert_eq!(verifier.get_max_gas(), 1000);
    assert_eq!(verifier.get_remaining_gas(), 500);
}

#[test]
fn test_mock_verifier_should_abort_false() {
    let verifier = MockVerifier::new(1000, true).with_consumed(500);
    assert!(!verifier.should_abort());
}

#[test]
fn test_mock_verifier_should_abort_true() {
    let verifier = MockVerifier::new(1000, true).with_consumed(1000);
    assert!(verifier.should_abort());
}

#[test]
fn test_mock_verifier_should_abort_exceeded() {
    let verifier = MockVerifier::new(1000, true).with_consumed(1500);
    assert!(verifier.should_abort());
}

#[test]
fn test_remaining_gas_saturating() {
    let verifier = MockVerifier::new(100, true).with_consumed(200);
    assert_eq!(verifier.get_remaining_gas(), 0);
}

#[test]
fn test_mock_snapshot_height() {
    let snapshot = MockSnapshot::new(12345);
    assert_eq!(snapshot.height(), 12345);
}

#[test]
fn test_mock_snapshot_storage() {
    let snapshot = MockSnapshot::new(100).with_storage(vec![0x01, 0x02], vec![0xAA, 0xBB]);

    assert_eq!(snapshot.get_storage(&[0x01, 0x02]), Some(vec![0xAA, 0xBB]));
    assert_eq!(snapshot.get_storage(&[0x03, 0x04]), None);
}

#[test]
fn test_mock_snapshot_contains_transaction() {
    let tx_hash = UInt256::from_bytes(&[1u8; 32]).unwrap_or_default();
    let snapshot = MockSnapshot::new(100).with_transaction(tx_hash);

    assert!(snapshot.contains_transaction(&tx_hash));
    assert!(!snapshot.contains_transaction(&UInt256::zero()));
}

#[test]
fn test_mock_snapshot_contains_block() {
    let snapshot = MockSnapshot::new(100);
    assert!(!snapshot.contains_block(&UInt256::zero()));
}

#[test]
fn test_mock_snapshot_get_block_hash() {
    let snapshot = MockSnapshot::new(100);

    assert!(snapshot.get_block_hash(50).is_some());
    assert!(snapshot.get_block_hash(100).is_some());
    assert!(snapshot.get_block_hash(101).is_none());
}

#[test]
fn test_witness_as_trait_object() {
    fn accept_witness(w: &dyn Witness) -> usize {
        w.invocation_script().len() + w.verification_script().len()
    }

    let witness = MockWitness::new(vec![0x01, 0x02, 0x03], vec![0x04, 0x05]);
    assert_eq!(accept_witness(&witness), 5);
}

#[test]
fn test_verifier_as_trait_object() {
    fn accept_verifier(v: &dyn VerificationContext) -> i64 {
        v.get_remaining_gas()
    }

    let verifier = MockVerifier::new(1000, true).with_consumed(300);
    assert_eq!(accept_verifier(&verifier), 700);
}

#[test]
fn test_snapshot_as_trait_object() {
    fn accept_snapshot(s: &dyn BlockchainSnapshot) -> u32 {
        s.height()
    }

    let snapshot = MockSnapshot::new(42);
    assert_eq!(accept_snapshot(&snapshot), 42);
}

#[test]
fn test_verification_error_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<VerificationError>();
}

#[test]
fn test_verification_error_all_variants_eq() {
    let err1 = VerificationError::VerificationFailed {
        message: "test".to_string(),
    };
    let err2 = VerificationError::VerificationFailed {
        message: "test".to_string(),
    };
    let err3 = VerificationError::VerificationFailed {
        message: "other".to_string(),
    };
    assert_eq!(err1, err2);
    assert_ne!(err1, err3);

    let err4 = VerificationError::GasLimitExceeded {
        consumed: 100,
        max: 50,
    };
    let err5 = VerificationError::GasLimitExceeded {
        consumed: 100,
        max: 50,
    };
    assert_eq!(err4, err5);
    assert_ne!(err1, err4);

    let err6 = VerificationError::InvalidScript {
        message: "bad".to_string(),
    };
    let err7 = VerificationError::InvalidScript {
        message: "bad".to_string(),
    };
    assert_eq!(err6, err7);

    let err8 = VerificationError::InvalidSignature {
        message: "wrong".to_string(),
    };
    let err9 = VerificationError::InvalidSignature {
        message: "wrong".to_string(),
    };
    assert_eq!(err8, err9);

    let err10 = VerificationError::MissingWitness {
        hash: "0x123".to_string(),
    };
    let err11 = VerificationError::MissingWitness {
        hash: "0x123".to_string(),
    };
    assert_eq!(err10, err11);
}

#[test]
fn test_verification_error_debug_all_variants() {
    let err1 = VerificationError::verification_failed("msg");
    assert!(format!("{err1:?}").contains("VerificationFailed"));

    let err2 = VerificationError::gas_limit_exceeded(200, 100);
    assert!(format!("{err2:?}").contains("GasLimitExceeded"));

    let err3 = VerificationError::invalid_script("script error");
    assert!(format!("{err3:?}").contains("InvalidScript"));

    let err4 = VerificationError::invalid_signature("sig error");
    assert!(format!("{err4:?}").contains("InvalidSignature"));

    let err5 = VerificationError::missing_witness(&UInt160::zero());
    assert!(format!("{err5:?}").contains("MissingWitness"));
}

#[test]
fn test_mock_verifier_error_result() {
    struct FailingVerifier;

    impl VerificationContext for FailingVerifier {
        fn verify_witness(
            &self,
            _hash: &UInt160,
            _witness: &dyn Witness,
        ) -> VerificationResult<bool> {
            Err(VerificationError::gas_limit_exceeded(500, 100))
        }

        fn get_gas_consumed(&self) -> i64 {
            500
        }

        fn get_max_gas(&self) -> i64 {
            100
        }
    }

    let verifier = FailingVerifier;
    let hash = UInt160::zero();
    let witness = MockWitness::new(vec![], vec![]);
    let result = verifier.verify_witness(&hash, &witness);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        VerificationError::GasLimitExceeded { .. }
    ));
}

#[test]
fn test_snapshot_with_multiple_blocks() {
    let snapshot = MockSnapshot::new(1000)
        .with_storage(vec![1, 2], vec![10, 20])
        .with_storage(vec![3, 4], vec![30, 40]);

    assert_eq!(snapshot.get_storage(&[1, 2]), Some(vec![10, 20]));
    assert_eq!(snapshot.get_storage(&[3, 4]), Some(vec![30, 40]));
    assert!(snapshot.get_block_hash(500).is_some());
    assert!(snapshot.get_block_hash(1000).is_some());
    assert!(snapshot.get_block_hash(1001).is_none());
}

#[test]
fn test_verification_result_type_alias() {
    fn returns_verification_result() -> VerificationResult<i32> {
        Ok(42)
    }

    fn returns_verification_error() -> VerificationResult<i32> {
        Err(VerificationError::verification_failed("test"))
    }

    assert_eq!(returns_verification_result().unwrap(), 42);
    assert!(returns_verification_error().is_err());
}

#[test]
fn test_gas_edge_cases() {
    let zero_gas = MockVerifier::new(0, true).with_consumed(0);
    assert!(zero_gas.should_abort());
    assert_eq!(zero_gas.get_remaining_gas(), 0);

    let large_gas = MockVerifier::new(i64::MAX, true).with_consumed(i64::MAX / 2);
    assert!(!large_gas.should_abort());
    assert_eq!(large_gas.get_remaining_gas(), i64::MAX - i64::MAX / 2);
}
