use super::*;
use crate::{HardforkTableIdentity, OpCode, ProtocolIdentity, ProtocolVersion};
use neo_primitives::TriggerType;
use std::sync::{Arc, Barrier};

fn key(discriminator: u8) -> ExecutionPlanKey {
    ExecutionPlanKey::new(
        Arc::<[u8]>::from(vec![
            OpCode::PUSHDATA1.byte(),
            1,
            discriminator,
            OpCode::RET.byte(),
        ]),
        0,
        ProtocolIdentity::new(0x334f_454e, ProtocolVersion::NEO_N3_V3_10_1),
        HardforkTableIdentity::unconfigured(),
        TriggerType::APPLICATION,
        None,
    )
}

fn new_cache(max_entries: usize, max_bytes: usize) -> ExecutionPlanCache {
    ExecutionPlanCache::new(
        ExecutionPlanCacheLimits {
            max_entries,
            max_bytes,
        },
        ExecutionPlanLimits::default(),
    )
}

#[test]
fn concurrent_miss_is_single_flight_and_returns_one_plan() {
    const WORKERS: usize = 16;
    let cache = Arc::new(new_cache(8, 1024 * 1024));
    let key = key(0x42);
    let barrier = Arc::new(Barrier::new(WORKERS));

    let plans = std::thread::scope(|scope| {
        let handles = (0..WORKERS)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let key = key.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    cache.get_or_build(key).expect("single-flight plan")
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("worker"))
            .collect::<Vec<_>>()
    });

    assert!(plans.iter().all(|plan| Arc::ptr_eq(plan, &plans[0])));
    let snapshot = cache.snapshot();
    assert_eq!(snapshot.builds, 1);
    assert_eq!(snapshot.misses, 1);
    assert_eq!(snapshot.hits + snapshot.waits, (WORKERS - 1) as u64);
    assert_eq!(snapshot.ready_entries, 1);
    assert_eq!(snapshot.in_flight_entries, 0);
    assert_eq!(snapshot.reserved_bytes, 0);
}

#[test]
fn entry_and_byte_bounds_fail_closed_or_evict_ready_plans() {
    let cache = new_cache(1, 1024 * 1024);
    let first = cache.get_or_build(key(1)).expect("first plan");
    let second = cache.get_or_build(key(2)).expect("second plan");
    assert!(!Arc::ptr_eq(&first, &second));
    let snapshot = cache.snapshot();
    assert_eq!(snapshot.ready_entries, 1);
    assert_eq!(snapshot.evictions, 1);
    assert!(snapshot.ready_bytes <= 1024 * 1024);

    let disabled = new_cache(0, 0);
    assert!(matches!(
        disabled.get_or_build(key(3)),
        Err(ExecutionPlanCacheError::Capacity)
    ));
    assert_eq!(disabled.snapshot().capacity_rejections, 1);

    let no_bytes = new_cache(4, 1);
    assert!(matches!(
        no_bytes.get_or_build(key(4)),
        Err(ExecutionPlanCacheError::Capacity)
    ));
    assert_eq!(no_bytes.snapshot().builds, 0);
}

#[test]
fn failed_construction_is_not_published_or_retained() {
    let cache = new_cache(4, 1024 * 1024);
    let invalid = ExecutionPlanKey::new(
        Arc::<[u8]>::from(vec![OpCode::JMP.byte(), 1]),
        0,
        ProtocolIdentity::new(0x334f_454e, ProtocolVersion::NEO_N3_V3_10_1),
        HardforkTableIdentity::unconfigured(),
        TriggerType::APPLICATION,
        None,
    );

    assert!(matches!(
        cache.get_or_build(invalid.clone()),
        Err(ExecutionPlanCacheError::Build(_))
    ));
    assert!(matches!(
        cache.get_or_build(invalid),
        Err(ExecutionPlanCacheError::Build(_))
    ));
    let snapshot = cache.snapshot();
    assert_eq!(snapshot.ready_entries, 0);
    assert_eq!(snapshot.in_flight_entries, 0);
    assert_eq!(snapshot.builds, 2);
    assert_eq!(snapshot.build_failures, 2);
}

#[test]
fn cache_hit_reverifies_full_key_equality_and_reuses_plan() {
    let cache = new_cache(4, 1024 * 1024);
    let first = cache.get_or_build(key(9)).expect("first plan");
    let same = cache.get_or_build(key(9)).expect("same plan");
    let distinct = cache.get_or_build(key(10)).expect("distinct plan");

    assert!(Arc::ptr_eq(&first, &same));
    assert!(!Arc::ptr_eq(&first, &distinct));
    let snapshot = cache.snapshot();
    assert_eq!(snapshot.hits, 1);
    assert_eq!(snapshot.builds, 2);
    assert_eq!(snapshot.ready_entries, 2);
}
