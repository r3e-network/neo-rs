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
    // keep simulated attestation enabled even in sgx-hw builds.
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
            assert!(!service.verify_report(&report).unwrap());

            let result = service.verify_report_detailed(&report);
            assert_eq!(result, QuoteValidationResult::InvalidSignature);
        }
        Err(TeeError::FeatureNotEnabled(_)) => {}
        Err(other) => panic!("unexpected error: {other}"),
    }
}

#[cfg(feature = "sgx-hw")]
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

    assert_eq!(mrenclave1, mrenclave2);

    let different_binary = b"different binary";
    let mrenclave3 = service.compute_mrenclave(different_binary);
    assert_ne!(mrenclave1, mrenclave3);
}

#[test]
fn test_verify_quote_with_invalid_bytes() {
    let (_temp, service) = setup_service();

    let invalid_quote = vec![0u8; 100];
    let result = service.verify_quote(&invalid_quote);

    assert!(result.is_err());
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
