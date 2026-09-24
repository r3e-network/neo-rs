//! Behavioral tests for the async state-root service public API.
//!
//! Scope note: the Wave-2 prefetch worker is a work-in-progress subsystem gated
//! behind the `prefetch` feature and is not exercised here. These tests cover
//! only APIs available in the default build: trigger bookkeeping counters,
//! root verification, and metrics aggregation.

use std::sync::Arc;
use std::thread;

use neo_core::state_service::async_state_root::AsyncStateRootService;
use neo_primitives::UInt256;

fn hash(byte: u8) -> UInt256 {
    UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
}

#[test]
fn fresh_service_has_no_pending_or_cached_roots() {
    let service = AsyncStateRootService::new();

    assert_eq!(service.pending_count(), 0);
    assert_eq!(service.cached_count(), 0);
    assert!(service.try_get_root_at_height(0).is_none());
}

#[test]
fn verify_root_match_matches_documented_metrics() {
    let service = AsyncStateRootService::new();

    assert!(service.verify_root_match(100, hash(1), hash(1)));
    assert!(!service.verify_root_match(101, hash(2), hash(3)));

    let stats = service.get_metrics().snapshot();
    assert_eq!(stats.total_verified, 1);
    assert_eq!(stats.total_verification_failures, 1);
    assert_eq!(stats.total_triggered, 0);
    assert_eq!(stats.total_computed, 0);
}

#[test]
fn metrics_snapshot_is_cumulative() {
    let service = AsyncStateRootService::new();

    assert_eq!(service.get_metrics().snapshot().total_verified, 0);
    assert!(service.verify_root_match(7, hash(9), hash(9)));
    assert!(service.verify_root_match(8, hash(9), hash(9)));
    assert_eq!(service.get_metrics().snapshot().total_verified, 2);
}

#[test]
fn concurrent_verifications_are_all_counted() {
    let service = Arc::new(AsyncStateRootService::new());

    let handles: Vec<_> = (0..4)
        .map(|worker| {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                for height in 0..25u64 {
                    let expected = hash(worker as u8 + 1);
                    assert!(service.verify_root_match(height, expected, expected));
                    assert!(!service.verify_root_match(height, expected, hash(255)));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("verification thread panicked");
    }

    let stats = service.get_metrics().snapshot();
    assert_eq!(stats.total_verified, 100);
    assert_eq!(stats.total_verification_failures, 100);
}
