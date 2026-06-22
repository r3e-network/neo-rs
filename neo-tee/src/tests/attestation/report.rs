use super::*;

#[test]
fn test_simulated_report() {
    let report_data = [0x42u8; 64];
    let report = AttestationReport::simulated(report_data);

    assert_eq!(report.report_type, ReportType::Simulated);
    assert_eq!(report.report_data, report_data);
    assert!(report.verify());
}

#[test]
fn test_report_serialization() {
    let report = AttestationReport::simulated([0u8; 64]);
    let bytes = report.to_bytes();
    let restored = AttestationReport::from_bytes(&bytes).unwrap();

    assert_eq!(restored.mrenclave, report.mrenclave);
    assert_eq!(restored.mrsigner, report.mrsigner);
}

#[test]
fn test_timestamp_verification() {
    let mut report = AttestationReport::simulated([0u8; 64]);
    // Pin the timestamp a few seconds in the past so freshness checks are
    // deterministic regardless of how fast the test runs (a just-created
    // report has age 0 within the current second).
    report.timestamp = SystemTime::now() - Duration::from_secs(3);

    // Valid within the 24-hour and 10-second windows.
    assert!(report.verify_timestamp(Duration::from_secs(MAX_REPORT_AGE_SECONDS)));
    assert!(report.verify_timestamp(Duration::from_secs(10)));

    // A roughly 3-second-old report is rejected by tighter freshness windows.
    assert!(!report.verify_timestamp(Duration::from_secs(1)));
    assert!(!report.verify_timestamp(Duration::from_secs(0)));
}

#[test]
fn test_security_version_verification() {
    let mut report = AttestationReport::simulated([0u8; 64]);
    report.isv_svn = 5;

    assert!(report.verify_security_version(1));
    assert!(report.verify_security_version(5));
    assert!(!report.verify_security_version(6));
}

#[test]
fn test_mrenclave_verification() {
    let expected = [0x42u8; 32];
    let report = AttestationReport::simulated_with_measurements([0u8; 64], expected, [0u8; 32]);

    assert!(report.verify_mrenclave(&expected));
    assert!(!report.verify_mrenclave(&[0x00u8; 32]));
}

#[test]
fn test_mrsigner_verification() {
    let expected = [0x42u8; 32];
    let report = AttestationReport::simulated_with_measurements([0u8; 64], [0u8; 32], expected);

    assert!(report.verify_mrsigner(&expected));
    assert!(!report.verify_mrsigner(&[0x00u8; 32]));
}

#[test]
fn test_full_validation() {
    let mrenclave = [0x42u8; 32];
    let mrsigner = [0x43u8; 32];

    let report = AttestationReport::simulated_with_measurements([0u8; 64], mrenclave, mrsigner);

    let options = QuoteValidationOptions {
        expected_mrenclave: Some(mrenclave),
        expected_mrsigner: Some(mrsigner),
        min_isv_svn: 1,
        max_age: Duration::from_secs(MAX_REPORT_AGE_SECONDS),
        require_non_debug: false,
    };

    assert_eq!(report.validate(&options), QuoteValidationResult::Valid);

    // Wrong MRENCLAVE
    let wrong_options = QuoteValidationOptions {
        expected_mrenclave: Some([0x00u8; 32]),
        expected_mrsigner: Some(mrsigner),
        min_isv_svn: 1,
        max_age: Duration::from_secs(MAX_REPORT_AGE_SECONDS),
        require_non_debug: false,
    };
    assert_eq!(
        report.validate(&wrong_options),
        QuoteValidationResult::InvalidMrEnclave
    );
}

#[test]
fn test_quote_validation_result() {
    assert!(QuoteValidationResult::Valid.is_valid());
    assert!(!QuoteValidationResult::Expired.is_valid());
    assert!(!QuoteValidationResult::InvalidMrEnclave.is_valid());

    assert_eq!(QuoteValidationResult::Valid.description(), "Quote is valid");
    assert_eq!(
        QuoteValidationResult::Expired.description(),
        "Quote has expired"
    );
}

#[test]
fn test_quote_parse_too_short() {
    let short_bytes = vec![0u8; 100];
    assert!(Quote::from_bytes(&short_bytes).is_none());
}

#[test]
fn test_quote_parse_minimal() {
    // Create a minimal valid SGX quote v3 structure (436-byte fixed prefix).
    let mut bytes = vec![0u8; 436];

    // Set version to 3 (ECDSA)
    bytes[0] = 3;
    bytes[1] = 0;

    // Set signature type to ECDSA
    bytes[2] = 2;
    bytes[3] = 0;

    // Set ISV SVN to valid value (at offset 48 + 258)
    bytes[48 + 258] = 1;
    bytes[48 + 259] = 0;

    let quote = Quote::from_bytes(&bytes).unwrap();
    assert_eq!(quote.version, 3);
    assert_eq!(quote.signature_type, 2);
    assert_eq!(quote.isv_svn, 1);
}

#[test]
fn test_quote_parse_uses_sgx_v3_report_body_offsets() {
    let mut bytes = vec![0u8; 436];
    bytes[0] = 3;
    bytes[2] = 2;

    let mrenclave = [0x11u8; 32];
    let mrsigner = [0x22u8; 32];
    let report_data = [0x33u8; 64];

    bytes[48 + 64..48 + 96].copy_from_slice(&mrenclave);
    bytes[48 + 128..48 + 160].copy_from_slice(&mrsigner);
    bytes[48 + 320..48 + 384].copy_from_slice(&report_data);

    bytes[48 + 48] = 0x02; // debug bit
    bytes[48 + 256] = 0x34;
    bytes[48 + 257] = 0x12; // isv_prod_id = 0x1234
    bytes[48 + 258] = 0x78;
    bytes[48 + 259] = 0x56; // isv_svn = 0x5678

    let quote = Quote::from_bytes(&bytes).expect("quote should parse");
    assert_eq!(quote.mrenclave, mrenclave);
    assert_eq!(quote.mrsigner, mrsigner);
    assert_eq!(quote.report_data, report_data);
    assert_eq!(quote.attributes[0] & 0x02, 0x02);
    assert_eq!(quote.isv_prod_id, 0x1234);
    assert_eq!(quote.isv_svn, 0x5678);
}
