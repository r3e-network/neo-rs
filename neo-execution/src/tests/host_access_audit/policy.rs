use super::*;
use neo_manifest::CallFlags;
use neo_primitives::{FindOptions, Hardfork};
use neo_vm::{ContractResolutionIdentity, NativeCacheDomain, RangeDirection};
use std::mem::size_of;

fn key(suffix: &[u8]) -> StorageKey {
    StorageKey::new(17, suffix.to_vec())
}

fn resolution(hash: UInt160) -> ContractResolutionIdentity {
    ContractResolutionIdentity::new(hash, 17, 3, 0x1020_3040)
}

fn policy(declarations: Vec<HostAccessDeclaration>) -> HostAccessPolicy {
    HostAccessPolicy::try_new(declarations, HostAccessPolicyLimits::DEFAULT)
        .expect("bounded test policy")
}

#[test]
fn policy_matches_every_dependency_by_exact_identity() {
    let contract_hash = UInt160::from([0x42; 20]);
    let point = key(b"point");
    let range = StorageRangeAccess::prefix(
        17,
        b"range".to_vec(),
        RangeDirection::Reverse,
        FindOptions::None,
        16,
    );
    let cache = NativeCacheAccess::new(
        NativeCacheDomain {
            contract_hash,
            contract_id: -5,
            native_version: 1,
            partition: 7,
        },
        ResolvedNativeCacheScope::Entry(b"balance".to_vec()),
        NativeCacheAccessKind::Read,
    );
    let call = ContractCallAccess::new(
        ContractCallKind::Dynamic,
        resolution(contract_hash),
        12,
        "balanceOf",
        CallFlags::READ_STATES,
        1,
        1,
    );
    let notification = NotificationAccess::new(contract_hash, "Transfer", 3);
    let log = LogAccess::new(contract_hash, 32);
    let context = HostContextAccess::Hardfork(Hardfork::HfGorgon);
    let policy = policy(vec![
        HostAccessDeclaration::StorageRead(point.clone()),
        HostAccessDeclaration::StorageRange(range.clone()),
        HostAccessDeclaration::StorageWrite(StorageWriteAccess::new(point.clone(), 32)),
        HostAccessDeclaration::StorageDelete(point.clone()),
        HostAccessDeclaration::NativeCacheRead(cache.clone()),
        HostAccessDeclaration::ContractCall(call.clone()),
        HostAccessDeclaration::Notification(notification.clone()),
        HostAccessDeclaration::Log(log.clone()),
        HostAccessDeclaration::Witness(contract_hash),
        HostAccessDeclaration::Context(context),
        HostAccessDeclaration::FeeCharge(123),
    ]);
    let mut audit = HostAccessAudit::new(&policy);

    audit.storage_read(&point).expect("declared read");
    audit.storage_range(&range).expect("declared range");
    audit.storage_write(&point, 32).expect("declared write");
    audit.storage_delete(&point).expect("declared delete");
    audit
        .authorize_native_cache(&cache)
        .expect("declared native cache");
    audit.contract_call(&call).expect("declared call");
    audit
        .notification(contract_hash, "Transfer", 3)
        .expect("declared notification");
    audit.log(contract_hash, 5).expect("declared log");
    audit.witness(contract_hash).expect("declared witness");
    audit.context(context).expect("declared context");
    audit.fee(123).expect("declared fee");

    assert!(audit.is_clean());
    audit.finish().expect("all accesses were declared");
}

#[test]
fn first_undeclared_access_latches_and_later_declared_access_still_fails() {
    let allowed = key(b"allowed");
    let denied = key(b"denied");
    let policy = policy(vec![HostAccessDeclaration::StorageRead(allowed.clone())]);
    let mut audit = HostAccessAudit::new(&policy);

    let first = audit
        .storage_read(&denied)
        .expect_err("undeclared key must fail");
    let second = audit
        .storage_read(&allowed)
        .expect_err("latched audit must remain failed");

    assert_eq!(first, second);
    assert_eq!(
        first.attempted(),
        &HostAccessDeclaration::StorageRead(denied)
    );
    assert!(!audit.is_clean());
    assert_eq!(audit.violation(), Some(&first));
    assert_eq!(audit.finish(), Err(first));
}

#[test]
fn near_miss_call_range_log_and_context_shapes_are_rejected() {
    let contract_hash = UInt160::from([0x11; 20]);
    let declared_range = StorageRangeAccess::prefix(
        4,
        b"x".to_vec(),
        RangeDirection::Forward,
        FindOptions::None,
        2,
    );
    let declared_call = ContractCallAccess::new(
        ContractCallKind::Dynamic,
        resolution(contract_hash),
        8,
        "run",
        CallFlags::ALL,
        1,
        0,
    );
    let declared_log = LogAccess::new(contract_hash, 5);
    let policy = policy(vec![
        HostAccessDeclaration::StorageRange(declared_range),
        HostAccessDeclaration::ContractCall(declared_call),
        HostAccessDeclaration::Log(declared_log),
        HostAccessDeclaration::Context(HostContextAccess::InvocationCounter(contract_hash)),
    ]);

    let mut range_audit = HostAccessAudit::new(&policy);
    let wrong_range = StorageRangeAccess::prefix(
        4,
        b"x".to_vec(),
        RangeDirection::Reverse,
        FindOptions::None,
        2,
    );
    assert!(range_audit.storage_range(&wrong_range).is_err());

    let mut call_audit = HostAccessAudit::new(&policy);
    let wrong_call = ContractCallAccess::new(
        ContractCallKind::Dynamic,
        resolution(contract_hash),
        8,
        "run",
        CallFlags::READ_STATES,
        1,
        0,
    );
    assert!(call_audit.contract_call(&wrong_call).is_err());

    let mut log_audit = HostAccessAudit::new(&policy);
    assert!(log_audit.log(contract_hash, 6).is_err());

    let mut context_audit = HostAccessAudit::new(&policy);
    assert!(
        context_audit
            .context(HostContextAccess::InvocationCounter(UInt160::zero()))
            .is_err()
    );
}

#[test]
fn policy_construction_enforces_entry_and_byte_bounds() {
    let declaration = HostAccessDeclaration::StorageRead(key(b"bounded"));
    let entry_error = HostAccessPolicy::try_new(
        vec![declaration.clone(), declaration.clone()],
        HostAccessPolicyLimits {
            max_declarations: 1,
            max_bytes: usize::MAX,
        },
    )
    .expect_err("entry bound must fail before retaining the second entry");
    assert_eq!(
        entry_error,
        HostAccessPolicyError::DeclarationCapacity { maximum: 1 }
    );

    let minimum = size_of::<HostAccessDeclaration>() + b"bounded".len();
    let byte_error = HostAccessPolicy::try_new(
        vec![declaration],
        HostAccessPolicyLimits {
            max_declarations: 1,
            max_bytes: minimum - 1,
        },
    )
    .expect_err("byte bound must reject the declaration");
    assert_eq!(
        byte_error,
        HostAccessPolicyError::ByteCapacity {
            required: minimum,
            maximum: minimum - 1,
        }
    );
}

#[test]
fn payload_bounds_native_direction_and_contract_version_are_fail_closed() {
    let contract_hash = UInt160::from([0x55; 20]);
    let domain = NativeCacheDomain {
        contract_hash,
        contract_id: -4,
        native_version: 2,
        partition: 3,
    };
    let native_read = NativeCacheAccess::new(
        domain,
        ResolvedNativeCacheScope::WholeDomain,
        NativeCacheAccessKind::Read,
    );
    let call = ContractCallAccess::new(
        ContractCallKind::Dynamic,
        resolution(contract_hash),
        9,
        "run",
        CallFlags::ALL,
        0,
        1,
    );
    let policy = policy(vec![
        HostAccessDeclaration::NativeCacheRead(native_read.clone()),
        HostAccessDeclaration::ContractCall(call),
        HostAccessDeclaration::Notification(NotificationAccess::new(contract_hash, "Event", 2)),
    ]);

    let native_write = NativeCacheAccess::new(
        domain,
        ResolvedNativeCacheScope::WholeDomain,
        NativeCacheAccessKind::Write,
    );
    assert!(
        HostAccessAudit::new(&policy)
            .authorize_native_cache(&native_write)
            .is_err()
    );
    assert!(
        HostAccessAudit::new(&policy)
            .notification(contract_hash, "Event", 3)
            .is_err()
    );

    let updated_target = ContractResolutionIdentity::new(contract_hash, 17, 4, 0x1020_3040);
    let updated_call = ContractCallAccess::new(
        ContractCallKind::Dynamic,
        updated_target,
        9,
        "run",
        CallFlags::ALL,
        0,
        1,
    );
    assert!(
        HostAccessAudit::new(&policy)
            .contract_call(&updated_call)
            .is_err()
    );
}
