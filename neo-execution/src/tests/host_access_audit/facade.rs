use super::*;
use crate::NoDiagnostic;
use crate::host_access_audit::{
    HostAccessDeclaration, HostAccessPolicy, HostAccessPolicyLimits, LogAccess, NotificationAccess,
    StorageWriteAccess,
};
use crate::native_contract_provider::NoNativeContractProvider;
use neo_config::ProtocolSettings;
use neo_manifest::CallFlags;
use neo_primitives::{FindOptions, TriggerType};
use neo_storage::{DataCache, StorageItem};
use neo_vm::{ContractResolutionIdentity, OpCode, RangeDirection, StackItem};

fn policy(declarations: Vec<HostAccessDeclaration>) -> HostAccessPolicy {
    HostAccessPolicy::try_new(declarations, HostAccessPolicyLimits::DEFAULT)
        .expect("bounded test policy")
}

fn engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            1_000_000,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("engine builds");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("script loads");
    engine
}

#[test]
fn denied_storage_write_is_rejected_before_mutating_the_snapshot() {
    let snapshot = Arc::new(DataCache::new(false));
    let storage_key = StorageKey::new(7, b"key".to_vec());
    snapshot.add(
        storage_key.clone(),
        StorageItem::from_bytes(b"before".to_vec()),
    );
    let mut engine = engine(Arc::clone(&snapshot));
    let policy = policy(vec![HostAccessDeclaration::StorageRead(
        storage_key.clone(),
    )]);
    let mut audit = HostAccessAudit::new(&policy);

    let error = AuditedApplicationHost::new(&mut engine, &mut audit)
        .storage_put(&storage_key, b"after".to_vec())
        .expect_err("undeclared write must fail");

    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert_eq!(
        snapshot
            .get(&storage_key)
            .expect("original value remains")
            .value_bytes()
            .as_ref(),
        b"before"
    );
    assert_eq!(
        audit.violation().expect("violation latched").attempted(),
        &HostAccessDeclaration::StorageWrite(StorageWriteAccess::new(storage_key, b"after".len(),))
    );
}

#[test]
fn oversized_declared_storage_write_is_rejected_before_fee_or_snapshot_effects() {
    let snapshot = Arc::new(DataCache::new(false));
    let storage_key = StorageKey::new(7, b"bounded".to_vec());
    snapshot.add(
        storage_key.clone(),
        StorageItem::from_bytes(b"before".to_vec()),
    );
    let mut engine = engine(Arc::clone(&snapshot));
    let policy = policy(vec![
        HostAccessDeclaration::StorageRead(storage_key.clone()),
        HostAccessDeclaration::StorageWrite(StorageWriteAccess::new(storage_key.clone(), 3)),
    ]);
    let mut audit = HostAccessAudit::new(&policy);
    let fee_before = engine.fee_consumed_pico();

    let error = AuditedApplicationHost::new(&mut engine, &mut audit)
        .storage_put(&storage_key, b"after".to_vec())
        .expect_err("value beyond the declared bound must fail");

    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert_eq!(engine.fee_consumed_pico(), fee_before);
    assert_eq!(
        snapshot
            .get(&storage_key)
            .expect("original value remains")
            .value_bytes()
            .as_ref(),
        b"before"
    );
}

#[test]
fn declared_point_range_and_context_reads_use_the_ordinary_host() {
    let snapshot = Arc::new(DataCache::new(false));
    let first = StorageKey::new(7, b"prefix-a".to_vec());
    let second = StorageKey::new(7, b"prefix-b".to_vec());
    snapshot.add(first.clone(), StorageItem::from_bytes(b"one".to_vec()));
    snapshot.add(second, StorageItem::from_bytes(b"two".to_vec()));
    let mut engine = engine(snapshot);
    let range = StorageRangeAccess::prefix(
        7,
        b"prefix-".to_vec(),
        RangeDirection::Forward,
        FindOptions::None,
        2,
    );
    let policy = policy(vec![
        HostAccessDeclaration::StorageRead(first.clone()),
        HostAccessDeclaration::StorageRange(range.clone()),
        HostAccessDeclaration::Context(HostContextAccess::Network),
        HostAccessDeclaration::Context(HostContextAccess::Trigger),
        HostAccessDeclaration::Context(HostContextAccess::FeeWhitelist),
    ]);
    let mut audit = HostAccessAudit::new(&policy);
    {
        let mut host = AuditedApplicationHost::new(&mut engine, &mut audit);

        assert_eq!(
            host.storage_get(&first).expect("declared point read"),
            Some(b"one".to_vec())
        );
        let mut iterator = host.storage_find(&range).expect("declared range read");
        assert!(crate::iterators::iterator::StorageIterator::next(
            &mut iterator
        ));
        assert_eq!(
            host.network().expect("declared network"),
            ProtocolSettings::default().network
        );
        assert_eq!(
            host.trigger().expect("declared trigger"),
            TriggerType::Application
        );
        assert!(!host.fee_whitelisted().expect("declared fee mode"));
    }
    audit.finish().expect("all accesses declared");
}

#[test]
fn range_bound_and_unsupported_half_open_domain_fail_closed() {
    let snapshot = Arc::new(DataCache::new(false));
    snapshot.add(
        StorageKey::new(7, b"prefix-a".to_vec()),
        StorageItem::from_bytes(b"one".to_vec()),
    );
    snapshot.add(
        StorageKey::new(7, b"prefix-b".to_vec()),
        StorageItem::from_bytes(b"two".to_vec()),
    );
    let mut engine = engine(snapshot);
    let bounded = StorageRangeAccess::prefix(
        7,
        b"prefix-".to_vec(),
        RangeDirection::Forward,
        FindOptions::None,
        1,
    );
    let half_open = StorageRangeAccess::half_open(
        7,
        b"prefix-a".to_vec(),
        b"prefix-z".to_vec(),
        RangeDirection::Forward,
        FindOptions::None,
        2,
    );
    let policy = policy(vec![
        HostAccessDeclaration::StorageRange(bounded.clone()),
        HostAccessDeclaration::StorageRange(half_open.clone()),
    ]);

    let mut bounded_audit = HostAccessAudit::new(&policy);
    let error = AuditedApplicationHost::new(&mut engine, &mut bounded_audit)
        .storage_find(&bounded)
        .expect_err("more than max_items must fall back");
    assert!(matches!(error, AuditedHostError::Host(_)));
    assert!(bounded_audit.is_clean());

    let mut half_open_audit = HostAccessAudit::new(&policy);
    let error = AuditedApplicationHost::new(&mut engine, &mut half_open_audit)
        .storage_find(&half_open)
        .expect_err("unsupported range primitive must fall back");
    assert!(matches!(error, AuditedHostError::Host(_)));
    assert!(half_open_audit.is_clean());
}

#[test]
fn fee_and_witness_attempts_are_exact_and_fail_closed() {
    let mut engine = engine(Arc::new(DataCache::new(false)));
    let witness = UInt160::from([0x33; 20]);
    let policy = policy(vec![
        HostAccessDeclaration::FeeCharge(5),
        HostAccessDeclaration::CpuFeeCharge(7),
        HostAccessDeclaration::Witness(witness),
    ]);
    let mut fee_audit = HostAccessAudit::new(&policy);
    let fee_before = engine.fee_consumed_pico();

    let error = AuditedApplicationHost::new(&mut engine, &mut fee_audit)
        .charge_execution_fee(6)
        .expect_err("different fee must be rejected");
    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert_eq!(engine.fee_consumed_pico(), fee_before);

    let mut cpu_fee_audit = HostAccessAudit::new(&policy);
    let error = AuditedApplicationHost::new(&mut engine, &mut cpu_fee_audit)
        .charge_cpu_fee_units(8)
        .expect_err("different CPU fee must be rejected");
    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert_eq!(engine.fee_consumed_pico(), fee_before);

    let mut declared_cpu_fee_audit = HostAccessAudit::new(&policy);
    AuditedApplicationHost::new(&mut engine, &mut declared_cpu_fee_audit)
        .charge_cpu_fee_units(7)
        .expect("declared CPU fee uses the Policy factor");
    assert_eq!(engine.fee_consumed_pico() - fee_before, 7 * 300_000);

    let mut witness_audit = HostAccessAudit::new(&policy);
    let stranger = UInt160::from([0x44; 20]);
    assert!(
        AuditedApplicationHost::new(&mut engine, &mut witness_audit)
            .check_witness(&stranger)
            .is_err()
    );
    assert_eq!(
        witness_audit
            .violation()
            .expect("witness violation")
            .attempted(),
        &HostAccessDeclaration::Witness(stranger)
    );
}

#[test]
fn native_origin_call_rejects_an_undeclared_calling_hash_before_effects() {
    let mut engine = engine(Arc::new(DataCache::new(false)));
    let target = UInt160::from([0x55; 20]);
    let declared_caller = UInt160::from([0x66; 20]);
    let actual_caller = UInt160::from([0x77; 20]);
    let call = ContractCallAccess::new(
        ContractCallKind::FromNativeVoid,
        ContractResolutionIdentity::new(target, 17, 1, 0x1020_3040),
        4,
        "callback",
        CallFlags::ALL,
        0,
        0,
    )
    .with_native_calling_script_hash(declared_caller);
    let policy = policy(vec![HostAccessDeclaration::ContractCall(call.clone())]);
    let mut audit = HostAccessAudit::new(&policy);
    let fee_before = engine.fee_consumed_pico();
    let depth_before = engine.invocation_stack().len();

    let error = AuditedApplicationHost::new(&mut engine, &mut audit)
        .call_from_native_void(&actual_caller, &call, Vec::new())
        .expect_err("a different native caller must be undeclared");

    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert_eq!(engine.fee_consumed_pico(), fee_before);
    assert_eq!(engine.invocation_stack().len(), depth_before);
    let HostAccessDeclaration::ContractCall(attempted) = audit
        .violation()
        .expect("call mismatch latched")
        .attempted()
    else {
        panic!("expected a contract-call violation");
    };
    assert_eq!(attempted.native_calling_script_hash(), Some(actual_caller));
}

#[test]
fn notification_and_log_bounds_are_checked_before_emission() {
    let mut engine = engine(Arc::new(DataCache::new(false)));
    let emitter = engine
        .current_script_hash()
        .expect("loaded script has a logical hash");
    let policy = policy(vec![
        HostAccessDeclaration::Notification(NotificationAccess::new(emitter, "Event", 1)),
        HostAccessDeclaration::Log(LogAccess::new(emitter, 4)),
    ]);

    let mut notification_audit = HostAccessAudit::new(&policy);
    let error = AuditedApplicationHost::new(&mut engine, &mut notification_audit)
        .send_notification(
            emitter,
            "Event",
            vec![StackItem::from_bool(true), StackItem::from_bool(false)],
        )
        .expect_err("oversized notification shape must fail");
    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert!(engine.notifications().is_empty());

    let mut log_audit = HostAccessAudit::new(&policy);
    let error = AuditedApplicationHost::new(&mut engine, &mut log_audit)
        .log("12345".to_string())
        .expect_err("oversized log must fail");
    assert!(matches!(error, AuditedHostError::Undeclared(_)));
    assert!(engine.logs().is_empty());
}
