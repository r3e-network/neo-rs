use super::*;
use crate::enclave::EnclaveConfig;
use tempfile::tempdir;

fn setup_service() -> (tempfile::TempDir, AttestationService) {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();

    // Test helper always uses a simulation enclave, so force testing config to
    // keep simulated attestation enabled even in tee-sgx builds.
    let service = AttestationService::with_config(enclave, AttestationConfig::testing()).unwrap();
    (temp, service)
}

fn setup_service_with_config(config: AttestationConfig) -> (tempfile::TempDir, AttestationService) {
    let temp = tempdir().unwrap();
    let enclave_config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(enclave_config));
    enclave.initialize().unwrap();

    let service = AttestationService::with_config(enclave, config).unwrap();
    (temp, service)
}

#[test]
fn test_generate_and_verify_report() {
    let (_temp, service) = setup_service();

    let user_data = b"test attestation data";
    let report = service.generate_report(user_data).unwrap();

    assert!(service.verify_report(&report).unwrap());
}

#[test]
fn test_key_attestation() {
    let (_temp, service) = setup_service();

    let public_key = [0x02u8; 33]; // Compressed public key
    let report = service.attest_key(&public_key).unwrap();

    assert!(service.verify_report(&report).unwrap());
}

#[test]
fn test_enclave_identity_verification() {
    let (_temp, service) = setup_service();

    let report = service.generate_report(b"test").unwrap();

    // Should match expected values for simulation
    let expected_mr = service.expected_mrenclave();
    assert!(service.verify_enclave_identity(&report, Some(&expected_mr), None));
}

#[test]
fn test_mrenclave_mismatch() {
    let (_temp, service) = setup_service();

    let report = service.generate_report(b"test").unwrap();

    let wrong_mrenclave = [0xFFu8; 32];
    let result = service.verify_mrenclave(&report, &wrong_mrenclave);

    assert!(result.is_err());
    match result.unwrap_err() {
        TeeError::MrEnclaveMismatch { expected, actual } => {
            assert_eq!(expected, hex::encode(wrong_mrenclave));
            assert_eq!(actual, hex::encode(report.mrenclave));
        }
        _ => panic!("Expected MrEnclaveMismatch error"),
    }
}

#[test]
fn test_mrsigner_mismatch() {
    let (_temp, service) = setup_service();

    let report = service.generate_report(b"test").unwrap();

    let wrong_mrsigner = [0xFFu8; 32];
    let result = service.verify_mrsigner(&report, &wrong_mrsigner);

    assert!(result.is_err());
    match result.unwrap_err() {
        TeeError::MrSignerMismatch { expected, actual } => {
            assert_eq!(expected, hex::encode(wrong_mrsigner));
            assert_eq!(actual, hex::encode(report.mrsigner));
        }
        _ => panic!("Expected MrSignerMismatch error"),
    }
}

#[test]
fn test_production_config_rejects_simulated() {
    let config = AttestationConfig::production();
    let (_temp, service) = setup_service_with_config(config);

    match service.generate_report(b"test") {
        Ok(report) => {
            // Should fail because simulated reports are not allowed in production config.
            assert!(!service.verify_report(&report).unwrap());

            // Detailed check should return InvalidSignature.
            let result = service.verify_report_detailed(&report);
            assert_eq!(result, QuoteValidationResult::InvalidSignature);
        }
        Err(TeeError::FeatureNotEnabled(_)) => {
            // In strict SGX mode we fail closed until real quote generation is integrated.
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

#[cfg(feature = "tee-sgx")]
#[test]
fn test_strict_sgx_mode_fails_closed_when_quote_generation_unavailable() {
    let config = AttestationConfig::production();
    let (_temp, service) = setup_service_with_config(config);

    match service.generate_report(b"strict-mode-test") {
        Err(TeeError::FeatureNotEnabled(_)) => {}
        Ok(_) => panic!("strict SGX mode must not emit simulated attestation reports"),
        Err(other) => panic!("unexpected error: {other}"),
    }
}

#[test]
fn test_config_with_mrenclave() {
    let expected_mrenclave = [0x42u8; 32];
    let config = AttestationConfig::testing().with_mrenclave(expected_mrenclave);

    assert_eq!(config.expected_mrenclave, Some(expected_mrenclave));
}

#[test]
fn test_compute_mrenclave() {
    let (_temp, service) = setup_service();

    let binary = b"test enclave binary";
    let mrenclave1 = service.compute_mrenclave(binary);
    let mrenclave2 = service.compute_mrenclave(binary);

    // Same binary should produce same MRENCLAVE
    assert_eq!(mrenclave1, mrenclave2);

    // Different binary should produce different MRENCLAVE
    let different_binary = b"different binary";
    let mrenclave3 = service.compute_mrenclave(different_binary);
    assert_ne!(mrenclave1, mrenclave3);

    // Verify MRENCLAVE is properly formed (32 bytes, not all zeros)
    assert_eq!(mrenclave1.len(), 32);
    assert!(mrenclave1.iter().any(|&b| b != 0));
}

#[test]
fn test_compute_mrenclave_with_metadata() {
    let (_temp, service) = setup_service();

    let binary = b"test enclave code";

    // Production metadata (non-debug)
    let prod_metadata = EnclaveMetadata::production(1024 * 1024 * 64);
    let prod_mrenclave = service.compute_mrenclave_with_metadata(binary, &prod_metadata);

    // Debug metadata
    let debug_metadata = EnclaveMetadata::debug(1024 * 1024 * 64);
    let debug_mrenclave = service.compute_mrenclave_with_metadata(binary, &debug_metadata);

    // Different metadata should produce different MRENCLAVE
    assert_ne!(prod_mrenclave, debug_mrenclave);

    // Same metadata should produce same result
    let prod_mrenclave2 = service.compute_mrenclave_with_metadata(binary, &prod_metadata);
    assert_eq!(prod_mrenclave, prod_mrenclave2);
}

#[test]
fn test_mrenclave_page_boundary_handling() {
    let (_temp, service) = setup_service();

    // Test with exactly 1 page (4096 bytes)
    let one_page = vec![0xAAu8; 4096];
    let mrenclave_1page = service.compute_mrenclave(&one_page);

    // Test with 1 page + 1 byte (triggers second page)
    let one_page_plus = vec![0xAAu8; 4097];
    let mrenclave_1page_plus = service.compute_mrenclave(&one_page_plus);

    // Should be different due to page boundary
    assert_ne!(mrenclave_1page, mrenclave_1page_plus);

    // Test with partial page
    let partial_page = vec![0xBBu8; 256];
    let mrenclave_partial = service.compute_mrenclave(&partial_page);

    // Should produce valid measurement
    assert!(mrenclave_partial.iter().any(|&b| b != 0));
}

#[test]
fn test_mrenclave_chunk_processing() {
    let (_temp, service) = setup_service();

    // Test that 256-byte chunks are processed correctly
    // Each chunk is hashed via EEXTEND
    let chunk_aligned = vec![0x42u8; 256];
    let mrenclave1 = service.compute_mrenclave(&chunk_aligned);

    // Same data in different alignment should still process correctly
    let mut chunk_unaligned = vec![0x00u8; 128];
    chunk_unaligned.extend_from_slice(&[0x42u8; 256]);
    let mrenclave2 = service.compute_mrenclave(&chunk_unaligned);

    // Different due to different total content
    assert_ne!(mrenclave1, mrenclave2);
}

#[test]
fn test_enclave_metadata_defaults() {
    let metadata = EnclaveMetadata::default();

    assert_eq!(metadata.ssa_frame_size, 1);
    assert!(metadata.enclave_size > 0);
    assert_eq!(metadata.misc_select, 0);

    // Default should be 64-bit mode
    assert_eq!(metadata.attributes[0] & 0x04, 0x04);

    // Default should not be debug mode
    assert_eq!(metadata.attributes[0] & 0x02, 0x00);
}

#[test]
fn test_production_vs_debug_metadata() {
    let prod = EnclaveMetadata::production(1024 * 1024 * 128);
    let debug = EnclaveMetadata::debug(1024 * 1024 * 128);

    // Production should not have DEBUG bit set
    assert_eq!(prod.attributes[0] & 0x02, 0x00);

    // Debug should have DEBUG bit set
    assert_eq!(debug.attributes[0] & 0x02, 0x02);

    // Both should have MODE64BIT
    assert_eq!(prod.attributes[0] & 0x04, 0x04);
    assert_eq!(debug.attributes[0] & 0x04, 0x04);
}

#[test]
fn test_verify_quote_with_invalid_bytes() {
    let (_temp, service) = setup_service();

    let invalid_quote = vec![0u8; 100]; // Too short
    let result = service.verify_quote(&invalid_quote);

    assert!(result.is_err());
}

#[cfg(not(feature = "tee-sgx"))]
#[test]
fn test_verify_quote_fails_closed_without_hardware_feature() {
    let (_temp, service) = setup_service();

    // Structurally valid SGX quote v3 prefix: every field check passes.
    let mut quote_bytes = vec![0u8; 436];
    quote_bytes[0] = 3; // version
    quote_bytes[2] = 2; // ECDSA signature type
    quote_bytes[48 + 258] = 1; // isv_svn = 1

    match service.verify_quote(&quote_bytes) {
        Err(TeeError::FeatureNotEnabled(message)) => {
            assert!(message.contains("tee-sgx"), "unexpected: {message}");
        }
        other => panic!("expected FeatureNotEnabled, got {other:?}"),
    }

    match service.verify_quote_detailed(&quote_bytes, None) {
        Err(TeeError::FeatureNotEnabled(message)) => {
            assert!(message.contains("tee-sgx"), "unexpected: {message}");
        }
        other => panic!("expected FeatureNotEnabled, got {other:?}"),
    }
}

#[test]
fn test_batch_verification() {
    let (_temp, service) = setup_service();

    let reports = vec![
        service.generate_report(b"test1").unwrap(),
        service.generate_report(b"test2").unwrap(),
        service.generate_report(b"test3").unwrap(),
    ];

    let results = service.verify_reports_batch(&reports);

    assert_eq!(results.len(), 3);
    for (_, result) in results {
        assert_eq!(result, QuoteValidationResult::Valid);
    }
}

#[test]
fn test_service_requires_initialized_enclave() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(config));
    // Don't initialize!

    let result = AttestationService::new(enclave);
    assert!(result.is_err());
}

#[test]
fn test_config_conversions() {
    let config = AttestationConfig::default();
    let options = config.to_quote_options();

    assert_eq!(options.min_isv_svn, config.min_isv_svn);
    assert_eq!(options.max_age, config.max_report_age);
    assert_eq!(options.require_non_debug, config.require_non_debug);
}
