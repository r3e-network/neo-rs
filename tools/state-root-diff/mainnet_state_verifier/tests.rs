use super::offline::OfflineBlockSource;
use super::*;

#[test]
fn batch_durability_is_explicit_and_defaults_to_per_block_sync() {
    let default_args = Args::try_parse_from(["mainnet-state-verifier"]).unwrap();
    assert!(!default_args.batch_durable);
    let batch_args = Args::try_parse_from(["mainnet-state-verifier", "--batch-durable"]).unwrap();
    assert!(batch_args.batch_durable);
}

#[test]
fn cache_ab_and_fresh_audit_are_explicit_opt_ins() {
    let default_args = Args::try_parse_from(["mainnet-state-verifier"]).unwrap();
    assert!(!default_args.disable_mpt_node_cache);
    assert!(!default_args.new_audit_at_checkpoint);
    let experiment = Args::try_parse_from([
        "mainnet-state-verifier",
        "--disable-mpt-node-cache",
        "--new-audit-at-checkpoint",
    ])
    .unwrap();
    assert!(experiment.disable_mpt_node_cache);
    assert!(experiment.new_audit_at_checkpoint);
}

#[test]
fn process_memory_monitor_observes_current_process() {
    let mut monitor = ProcessMemoryMonitor::new().expect("current-process monitor");
    monitor.sample().expect("second RSS sample");
    let profile = monitor.profile();
    assert!(profile.initial_rss_bytes > 0);
    assert!(profile.final_rss_bytes > 0);
    assert!(profile.peak_rss_bytes >= profile.initial_rss_bytes);
    assert!(profile.peak_rss_bytes >= profile.final_rss_bytes);
    assert_eq!(profile.samples, 2);
}

#[test]
fn batch_parser_accepts_out_of_order_responses() {
    let expected = HashMap::from([
        (1, ("block".to_owned(), "getblock" as &'static str)),
        (2, ("root".to_owned(), "getstateroot" as &'static str)),
    ]);
    let payload = json!([
        {"jsonrpc":"2.0", "id":2, "result":{"index":0}},
        {"jsonrpc":"2.0", "id":1, "result":"block"}
    ]);
    let values = parse_batch_response(payload, &expected).expect("valid batch");
    assert_eq!(values.get("block").and_then(Value::as_str), Some("block"));
    assert_eq!(
        values
            .get("root")
            .and_then(|root| root.get("index"))
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[test]
fn batch_parser_rejects_missing_duplicate_unknown_and_rpc_errors() {
    let expected = HashMap::from([(1, ("one".to_owned(), "getblock" as &'static str))]);
    for payload in [
        json!([]),
        json!([{"jsonrpc":"2.0", "id":2, "result":null}]),
        json!([{"jsonrpc":"2.0", "id":1, "error":{"code":-1,"message":"bad"}}]),
    ] {
        assert!(parse_batch_response(payload, &expected).is_err());
    }
    let expected = HashMap::from([(1, ("one".to_owned(), "getblock" as &'static str))]);
    let duplicate = json!([
        {"jsonrpc":"2.0", "id":1, "result":1},
        {"jsonrpc":"2.0", "id":1, "result":2}
    ]);
    assert!(parse_batch_response(duplicate, &expected).is_err());
}

#[test]
fn audit_resume_requires_the_exact_store_watermark() {
    let hash = UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
        .expect("hash");
    let root = UInt256::parse("0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8")
        .expect("root");
    let checkpoint = neo_validator::state::VerifiedPoint {
        height: 7,
        block_hash: hash,
        state_root: root,
    };
    let record = AuditRecord {
        height: 7,
        block_hash: hash.to_string(),
        state_root: root.to_string(),
        transaction_count: 2,
        receipts_verified: true,
        receipt_count: 4,
        offline_block_offset: None,
        offline_next_offset: None,
    };
    let mut file = tempfile::NamedTempFile::new().expect("audit file");
    serde_json::to_writer(file.as_file_mut(), &record).expect("write audit");
    file.as_file_mut().write_all(b"\n").expect("write newline");
    file.as_file_mut().flush().expect("flush audit");
    validate_audit_resume(file.as_file_mut(), Some(&checkpoint), true).expect("matching resume");
    assert!(validate_audit_resume(file.as_file_mut(), Some(&checkpoint), false).is_err());
    assert!(validate_audit_resume(file.as_file_mut(), None, true).is_err());

    let mut wrong_checkpoint = checkpoint.clone();
    wrong_checkpoint.height = 8;
    assert!(validate_audit_resume(file.as_file_mut(), Some(&wrong_checkpoint), true).is_err());
    assert!(validate_audit_resume_mode(file.as_file_mut(), Some(&checkpoint), true, true).is_err());
    let mut empty = tempfile::NamedTempFile::new().expect("empty audit");
    assert!(validate_audit_resume_mode(empty.as_file_mut(), None, true, true).is_err());
    validate_audit_resume_mode(empty.as_file_mut(), Some(&checkpoint), true, true)
        .expect("explicit fresh audit at a verified checkpoint");
    assert!(validate_audit_resume(empty.as_file_mut(), Some(&wrong_checkpoint), true).is_err());
}

fn sample_receipts() -> Vec<ExecutionReceipt> {
    vec![
        ExecutionReceipt {
            trigger: "OnPersist".to_owned(),
            vm_state: "HALT".to_owned(),
            gas_consumed: 0,
            has_exception: false,
        },
        ExecutionReceipt {
            trigger: "Application".to_owned(),
            vm_state: "FAULT".to_owned(),
            gas_consumed: 123_456,
            has_exception: true,
        },
    ]
}

#[test]
fn application_log_parser_is_strict_and_preserves_receipt_fields() {
    let hash = UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
        .expect("hash");
    let log = json!({
        "blockhash": hash.to_string(),
        "executions": [
            {"trigger":"OnPersist", "vmstate":"HALT", "gasconsumed":"0", "exception":null},
            {"trigger":"Application", "vmstate":"FAULT", "gasconsumed":123456, "exception":"failed"}
        ]
    });
    assert_eq!(
        parse_application_log(&log, hash).expect("valid application log"),
        sample_receipts()
    );
    let mut wrong_hash = log.clone();
    wrong_hash["blockhash"] = json!(UInt256::zero().to_string());
    assert!(parse_application_log(&wrong_hash, hash).is_err());
    assert!(parse_application_log(&json!({"blockhash":hash.to_string()}), hash).is_err());
    let mut invalid_exception = log.clone();
    invalid_exception["executions"][0]["exception"] = json!({"message":"failed"});
    assert!(parse_application_log(&invalid_exception, hash).is_err());
    let mut invalid_gas = log;
    invalid_gas["executions"][0]["gasconsumed"] = json!("not-a-number");
    assert!(parse_application_log(&invalid_gas, hash).is_err());
}

#[test]
fn receipt_comparison_rejects_field_order_count_and_availability_differences() {
    let expected = sample_receipts();
    assert_eq!(
        compare_execution_receipts(7, Some(&expected), Some(&expected))
            .expect("identical receipts"),
        2
    );
    for index in 0..expected.len() {
        for field in 0..4 {
            let mut different = expected.clone();
            match field {
                0 => different[index].trigger.push_str("Changed"),
                1 => different[index].vm_state = "NONE".to_owned(),
                2 => different[index].gas_consumed += 1,
                _ => different[index].has_exception = !different[index].has_exception,
            }
            assert!(compare_execution_receipts(7, Some(&expected), Some(&different)).is_err());
        }
    }
    let mut reversed = expected.clone();
    reversed.reverse();
    assert!(compare_execution_receipts(7, Some(&expected), Some(&reversed)).is_err());
    assert!(compare_execution_receipts(7, Some(&expected), Some(&expected[..1])).is_err());
    assert!(compare_execution_receipts(7, None, Some(&expected)).is_err());
    assert!(compare_execution_receipts(7, Some(&expected), None).is_err());
}

#[test]
fn upstream_receipt_disagreement_and_missing_logs_are_rejected() {
    let hash = UInt256::zero();
    let make_evidence = |executions| UpstreamEvidence {
        height: 7,
        raw_block: vec![1, 2, 3],
        state_root: hash,
        executions,
    };
    let primary = make_evidence(Some(sample_receipts()));
    let secondary = make_evidence(Some(sample_receipts()));
    assert!(compare_evidence(primary, secondary).is_ok());
    let primary = make_evidence(Some(sample_receipts()));
    let mut different = sample_receipts();
    different[1].gas_consumed += 1;
    let secondary = make_evidence(Some(different));
    assert!(compare_evidence(primary, secondary).is_err());
    let primary = make_evidence(Some(sample_receipts()));
    let secondary = make_evidence(None);
    assert!(compare_evidence(primary, secondary).is_err());
}

fn write_test_package(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("offline package");
    file.as_file_mut().write_all(bytes).expect("write package");
    file.as_file_mut().flush().expect("flush package");
    file
}

fn genesis_package() -> (tempfile::NamedTempFile, Vec<u8>, u64) {
    let raw = general_purpose::STANDARD.decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACI6hnvVQEAAB2sK3wAAAAAAAAAAABrEj3YvscYZIhSu8eFleNTagWPnwEAAREA").expect("genesis Base64");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&raw);
    let next_offset = bytes.len() as u64;
    (write_test_package(&bytes), raw, next_offset)
}

#[test]
fn offline_package_reader_matches_neo_cli_layout_and_resume_offsets() {
    let (package, raw, next_offset) = genesis_package();
    let mut source = OfflineBlockSource::open(package.path()).expect("open package");
    assert_eq!(source.start, 0);
    assert_eq!(source.end, 0);
    let batch = source.read_batch(0, 0).expect("read genesis");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].raw, raw);
    assert_eq!(batch[0].block_offset, 8);
    assert_eq!(batch[0].next_offset, next_offset);
    assert!(source.read_one().is_err());

    let mut source = OfflineBlockSource::open(package.path()).expect("reopen package");
    let checkpoint = neo_validator::state::VerifiedPoint {
        height: 0,
        block_hash: decode_block(&raw, 0).expect("genesis").hash(),
        state_root: UInt256::zero(),
    };
    let record = AuditRecord {
        height: 0,
        block_hash: checkpoint.block_hash.to_string(),
        state_root: checkpoint.state_root.to_string(),
        transaction_count: 0,
        receipts_verified: false,
        receipt_count: 0,
        offline_block_offset: Some(8),
        offline_next_offset: Some(next_offset),
    };
    source.resume(&checkpoint, &record).expect("exact resume");
    let mut wrong_record = record;
    wrong_record.offline_next_offset = Some(next_offset + 1);
    assert!(source.resume(&checkpoint, &wrong_record).is_err());
}

#[test]
fn offline_package_reader_rejects_bad_headers_truncation_and_trailing_bytes() {
    let (package, raw, _) = genesis_package();
    let mut bytes = std::fs::read(package.path()).expect("package bytes");
    bytes[0] = 1;
    let wrong_start = write_test_package(&bytes);
    assert!(OfflineBlockSource::open(wrong_start.path()).is_err());

    let mut bytes = std::fs::read(package.path()).expect("package bytes");
    bytes[4] = 2;
    let missing_block = write_test_package(&bytes);
    let mut source = OfflineBlockSource::open(missing_block.path()).expect("open count");
    assert!(source.read_batch(0, 1).is_err());

    let mut bytes = std::fs::read(package.path()).expect("package bytes");
    bytes.truncate(bytes.len() - 1);
    let truncated = write_test_package(&bytes);
    let mut source = OfflineBlockSource::open(truncated.path()).expect("open truncated");
    assert!(source.read_one().is_err());

    let mut bytes = std::fs::read(package.path()).expect("package bytes");
    bytes.push(0);
    let trailing = write_test_package(&bytes);
    let mut source = OfflineBlockSource::open(trailing.path()).expect("open trailing");
    assert!(source.read_one().is_err());

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let empty_block = write_test_package(&bytes);
    let mut source = OfflineBlockSource::open(empty_block.path()).expect("open empty");
    assert!(source.read_one().is_err());
    assert!(!raw.is_empty());
}

#[test]
fn genesis_raw_block_is_strictly_decoded_and_hashed() {
    let raw = general_purpose::STANDARD.decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACI6hnvVQEAAB2sK3wAAAAAAAAAAABrEj3YvscYZIhSu8eFleNTagWPnwEAAREA").expect("genesis Base64");
    let block = decode_block(&raw, 0).expect("genesis block");
    assert_eq!(
        block.hash().to_string(),
        "0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15"
    );
    assert!(decode_block(&raw, 1).is_err());
    let mut trailing = raw;
    trailing.push(0);
    assert!(decode_block(&trailing, 0).is_err());
}
