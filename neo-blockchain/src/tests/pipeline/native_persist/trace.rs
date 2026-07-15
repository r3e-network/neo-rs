use super::*;

#[test]
fn tx_filter_is_disabled_when_env_is_absent() {
    let tx_hash = UInt256::from([0x11; 32]);
    assert!(!TraceTxFilter::from_raw(None).matches(7, &tx_hash));
}

#[test]
fn tx_filter_matches_wildcards_and_listed_hashes() {
    let tx_hash = UInt256::from([0x22; 32]);
    let other_hash = UInt256::from([0x33; 32]);
    let raw = format!(" {tx_hash},not-a-match ");
    let filter = TraceTxFilter::from_raw(Some(&raw));

    assert!(filter.matches(7, &tx_hash));
    assert!(!filter.matches(7, &other_hash));
    assert!(TraceTxFilter::from_raw(Some(" all ")).matches(7, &other_hash));
    assert!(TraceTxFilter::from_raw(Some("*")).matches(7, &other_hash));
}

#[test]
fn tx_filter_matches_inclusive_block_ranges() {
    let tx_hash = UInt256::from([0x44; 32]);
    let filter = TraceTxFilter::from_raw_parts(None, Some("10-12"));

    assert!(!filter.matches(9, &tx_hash));
    assert!(filter.matches(10, &tx_hash));
    assert!(filter.matches(12, &tx_hash));
    assert!(!filter.matches(13, &tx_hash));
    assert!(!TraceTxFilter::from_raw_parts(None, Some("12-10")).matches(11, &tx_hash));
}

#[test]
fn slow_tx_filter_requires_valid_threshold_and_optional_range() {
    assert!(!SlowTxFilter::from_raw_parts(None, None).matches(10, 1_000));
    assert!(!SlowTxFilter::from_raw_parts(Some("0"), None).matches(10, 1_000));
    assert!(!SlowTxFilter::from_raw_parts(Some("bad"), None).matches(10, 1_000));

    let filter = SlowTxFilter::from_raw_parts(Some("500"), Some("10-12"));
    assert!(!filter.matches(9, 500));
    assert!(!filter.matches(10, 499));
    assert!(filter.matches(10, 500));
    assert!(filter.matches(12, 900));
    assert!(!filter.matches(13, 900));
}

#[test]
fn vm_profile_filter_targets_parsed_hashes_wildcards_and_ranges() {
    let tx_hash = UInt256::from([0x45; 32]);
    let other_hash = UInt256::from([0x46; 32]);
    let raw = format!("invalid, {tx_hash}");
    let filter = VmProfileFilter::from_raw_parts(Some(&raw), None);

    assert!(filter.matches(7, &tx_hash));
    assert!(!filter.matches(7, &other_hash));
    assert!(VmProfileFilter::from_raw_parts(Some("*"), None).matches(7, &other_hash));
    assert!(VmProfileFilter::from_raw_parts(None, Some("10-12")).matches(11, &other_hash));
    assert!(!VmProfileFilter::from_raw_parts(None, Some("12-10")).matches(11, &other_hash));
}

#[test]
fn vm_profile_formatters_emit_stable_class_and_hot_opcode_summaries() {
    let mut engine = neo_vm::ExecutionEngine::<()>::new(None);
    engine.enable_execution_profiling();
    engine
        .load_script(
            neo_vm::Script::new_relaxed(vec![
                neo_vm::OpCode::PUSH1.byte(),
                neo_vm::OpCode::PUSH2.byte(),
                neo_vm::OpCode::ADD.byte(),
                neo_vm::OpCode::RET.byte(),
            ]),
            -1,
            0,
        )
        .expect("load profiled script");
    assert_eq!(engine.execute(), neo_vm::VmState::HALT);
    let profile = engine.execution_profile().expect("execution profile");

    assert_eq!(
        format_vm_opcode_classes(&profile),
        "push=2,control_flow=1,numeric=1"
    );
    assert_eq!(
        format_vm_hottest_opcodes(&profile, 4),
        "PUSH1=1,PUSH2=1,RET=1,ADD=1"
    );
}

#[test]
fn tx_artifact_matches_application_log_shape() {
    let tx_hash = UInt256::from([0x55; 32]);
    let mut executed = neo_payloads::ApplicationExecuted::new(
        None,
        neo_primitives::TriggerType::APPLICATION,
        neo_vm::VmState::HALT,
        None,
        42,
        vec![neo_vm::StackItem::from_bool(true)],
    );
    executed
        .notifications
        .push(neo_payloads::NotifyEventArgs::new_with_optional_container(
            None,
            UInt160::from([0x66; 20]),
            "Transfer".to_string(),
            vec![neo_vm::StackItem::from_i64(7)],
        ));

    let artifact = trace_tx_artifact(12, &tx_hash, &executed).expect("render trace artifact");
    assert_eq!(artifact["block_index"], 12);
    assert_eq!(artifact["txid"], tx_hash.to_string());
    assert_eq!(artifact["executions"][0]["trigger"], "Application");
    assert_eq!(artifact["executions"][0]["vmstate"], "HALT");
    assert_eq!(artifact["executions"][0]["gasconsumed"], "42");
    assert_eq!(artifact["executions"][0]["stack"][0]["type"], "Boolean");
    assert_eq!(
        artifact["executions"][0]["notifications"][0]["state"]["value"][0]["value"],
        "7"
    );
}

#[test]
fn tx_filter_default_path_returns_before_hash_formatting() {
    let source = include_str!("../../../pipeline/native_persist/trace.rs");
    let matcher = source
        .split("fn matches(&self, block_index: u32, tx_hash: &UInt256) -> bool")
        .nth(1)
        .and_then(|tail| tail.split("fn trace_tx_frames").next())
        .expect("TraceTxFilter::matches source");
    let empty_guard = matcher
        .find("self.hashes.is_empty()")
        .expect("default no-trace guard should avoid hash formatting");
    let hash_format = matcher
        .find("tx_hash.to_string()")
        .expect("listed trace hashes still need string matching");
    assert!(empty_guard < hash_format);
}
