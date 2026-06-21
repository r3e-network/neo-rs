use super::*;
use tempfile::tempdir;

#[test]
fn test_enclave_lifecycle() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    assert_eq!(enclave.state(), EnclaveState::Uninitialized);

    let result = enclave.initialize().unwrap();
    assert_eq!(result.state, EnclaveState::Ready);
    assert!(result.sealing_key_derived);
    assert_eq!(enclave.state(), EnclaveState::Ready);
    assert!(enclave.is_ready());

    // Test monotonic counter
    let c1 = enclave.increment_counter().unwrap();
    let c2 = enclave.increment_counter().unwrap();
    assert_eq!(c2, c1 + 1);

    enclave.shutdown().unwrap();
    assert_eq!(enclave.state(), EnclaveState::Uninitialized);
}

#[test]
fn test_double_initialization() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    enclave.initialize().unwrap();

    // Second initialization should fail
    let result = enclave.initialize();
    assert!(result.is_err());

    match result.unwrap_err() {
        TeeError::EnclaveInitError { error, .. } => {
            assert_eq!(error, EnclaveInitError::AlreadyInitialized);
        }
        _ => panic!("Expected AlreadyInitialized error"),
    }
}

#[test]
fn test_invalid_configuration() {
    let temp = tempdir().unwrap();

    // Test invalid heap size
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        heap_size_mb: 0,
        simulation: true,
        ..Default::default()
    };
    let enclave = TeeEnclave::new(config);
    let result = enclave.initialize();
    assert!(result.is_err());

    // Test invalid TCS count
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        tcs_count: 0,
        simulation: true,
        ..Default::default()
    };
    let enclave = TeeEnclave::new(config);
    let result = enclave.initialize();
    assert!(result.is_err());
}

#[test]
fn test_debug_mode_restriction() {
    let temp = tempdir().unwrap();

    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        debug_mode: true,
        simulation: false, // Production mode
        allow_debug_in_production: false,
        ..Default::default()
    };
    let enclave = TeeEnclave::new(config);
    let result = enclave.initialize();
    assert!(result.is_err());

    // Allow debug mode
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        debug_mode: true,
        simulation: false,
        allow_debug_in_production: true,
        ..Default::default()
    };
    let enclave = TeeEnclave::new(config);
    // Will still fail in simulation mode without SGX device, but not due to debug restriction
    // Just check the error is NOT about debug mode
    let result = enclave.initialize();
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(!msg.contains("Debug mode not allowed"));
    }
}

#[cfg(feature = "sgx-hw")]
#[test]
fn test_sgx_hardware_mode_fails_closed_without_verified_evidence() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: false,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    let err = enclave
        .initialize()
        .expect_err("hardware mode must fail closed without verified SGX quote evidence");

    match err {
        TeeError::EnclaveInitError { error, .. } => {
            assert_eq!(error, EnclaveInitError::HardwareUnavailable);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[cfg(feature = "sgx-hw")]
#[test]
fn test_sgx_hardware_mode_accepts_real_evidence_when_opted_in() {
    if std::env::var("NEO_TEE_RUN_REAL_SGX_TEST").as_deref() != Ok("1") {
        // Opt-in only: requires operator-provided real SGX quote + sealing key evidence.
        return;
    }

    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: false,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    let init = enclave.initialize().expect(
        "real SGX test requires valid evidence (NEO_TEE_SGX_QUOTE_PATH + NEO_TEE_SGX_SEALING_KEY_PATH/HEX)",
    );
    assert!(init.hardware_attestation_available);
    assert!(enclave.is_ready());
}

#[test]
fn test_mrenclave_verification() {
    let temp = tempdir().unwrap();
    let expected = [0x42u8; 32];

    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true, // Simulation mode skips verification
        expected_mrenclave: Some(expected),
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    enclave.initialize().unwrap();

    // Correct MRENCLAVE should pass
    assert!(enclave.verify_mrenclave(&expected).is_ok());

    // Wrong MRENCLAVE should fail
    let wrong = [0x00u8; 32];
    let result = enclave.verify_mrenclave(&wrong);
    assert!(result.is_err());
    match result.unwrap_err() {
        TeeError::MrEnclaveMismatch { .. } => {}
        _ => panic!("Expected MrEnclaveMismatch error"),
    }
}

#[test]
fn test_mrsigner_verification() {
    let temp = tempdir().unwrap();
    let expected = [0x42u8; 32];

    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        expected_mrsigner: Some(expected),
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    enclave.initialize().unwrap();

    // Correct MRSIGNER should pass
    assert!(enclave.verify_mrsigner(&expected).is_ok());

    // Wrong MRSIGNER should fail
    let wrong = [0x00u8; 32];
    let result = enclave.verify_mrsigner(&wrong);
    assert!(result.is_err());
    match result.unwrap_err() {
        TeeError::MrSignerMismatch { .. } => {}
        _ => panic!("Expected MrSignerMismatch error"),
    }
}

#[test]
fn test_operations_before_init() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);

    // Operations should fail before initialization
    assert!(enclave.increment_counter().is_err());
    assert!(enclave.current_counter().is_err());
}

#[test]
fn test_config_validation() {
    // Valid config
    let config = EnclaveConfig {
        heap_size_mb: 256,
        tcs_count: 4,
        ..Default::default()
    };
    assert!(config.validate().is_ok());

    // Invalid heap size (too large)
    let config = EnclaveConfig {
        heap_size_mb: 5000,
        ..Default::default()
    };
    assert!(config.validate().is_err());

    // Invalid TCS count
    let config = EnclaveConfig {
        tcs_count: 300,
        ..Default::default()
    };
    assert!(config.validate().is_err());
}
