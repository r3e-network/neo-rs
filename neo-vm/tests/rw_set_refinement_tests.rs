//! Differential evidence only: this is not a proof of Rust refinement.
use neo_primitives::UInt256;
use neo_vm::RwSetTracker;
use std::collections::BTreeSet;

fn key(n: u8) -> UInt256 {
    UInt256::from_bytes(&[n; 32]).unwrap()
}

fn tracker(reads: &[u8], writes: &[u8]) -> RwSetTracker {
    let mut result = RwSetTracker::new();
    for &r in reads { result.record_read(key(r)); }
    for &w in writes { result.record_write(key(w)); }
    result
}

// Independent finite-set algebra, not the production Vec membership loop.
fn reference(writes: &[u8], other_reads: &[u8], other_writes: &[u8]) -> bool {
    let left: BTreeSet<_> = writes.iter().copied().collect();
    let reads: BTreeSet<_> = other_reads.iter().copied().collect();
    let writes: BTreeSet<_> = other_writes.iter().copied().collect();
    let right: BTreeSet<_> = reads.union(&writes).copied().collect();
    !left.is_disjoint(&right)
}

fn check(ar: &[u8], aw: &[u8], br: &[u8], bw: &[u8]) {
    let a = tracker(ar, aw);
    let b = tracker(br, bw);
    assert_eq!(a.conflicts_with(&b), reference(aw, br, bw));
    assert_eq!(b.conflicts_with(&a), reference(bw, ar, aw));
}

#[test]
fn empty() { check(&[], &[], &[], &[]); }
#[test]
fn read_read() { check(&[1], &[], &[1], &[]); }
#[test]
fn write_write() { check(&[], &[1], &[], &[1]); }
#[test]
fn write_read() { check(&[], &[1], &[1], &[]); }
#[test]
fn read_write_is_directional() {
    check(&[1], &[], &[], &[1]);
    let reader = tracker(&[1], &[]);
    let writer = tracker(&[], &[1]);
    assert!(!reader.conflicts_with(&writer));
    assert!(writer.conflicts_with(&reader));
}
#[test]
fn disjoint() { check(&[0, 1], &[2, 3], &[4], &[5]); }
#[test]
fn duplicate_accesses() {
    check(&[1, 1], &[2, 2], &[2, 2], &[3, 3]);
    let t = tracker(&[1, 1], &[2, 2]);
    assert_eq!(t.total_accesses(), 4); // count is a multiset observation
}
#[test]
fn order_does_not_affect_conflict() {
    check(&[3, 1], &[2, 0], &[0, 4], &[5]);
    check(&[1, 3], &[0, 2], &[4, 0], &[5]);
}
#[test]
fn self_conflict_requires_write() {
    for (r, w) in [(&[1][..], &[][..]), (&[][..], &[2][..])] {
        let t = tracker(r, w);
        assert_eq!(t.conflicts_with(&t), reference(w, r, w));
    }
}
#[test]
fn events_and_gas_are_not_conflict_keys() {
    let mut a = tracker(&[], &[]);
    a.record_notify_event(key(1));
    a.record_log_event(key(1));
    a.set_gas_used(-1);
    let b = tracker(&[], &[1]);
    assert_eq!(a.conflicts_with(&b), reference(&[], &[], &[1]));
    assert_eq!(b.conflicts_with(&a), reference(&[1], &[], &[]));
}
#[test]
fn clear_resets_all_observations() {
    let mut t = tracker(&[1], &[2]);
    t.record_notify_event(key(3));
    t.record_log_event(key(4));
    t.set_gas_used(10);
    t.clear();
    assert!(!t.has_reads() && !t.has_writes());
    assert_eq!(t.total_accesses(), 0);
    assert!(t.notify_events.is_empty() && t.log_events.is_empty());
    assert_eq!(t.gas_used, 0);
    assert_eq!(t.conflicts_with(&tracker(&[2], &[1])), reference(&[], &[2], &[1]));
}
#[test]
fn reserve_and_clone_preserve_conflicts() {
    let mut t = tracker(&[1], &[2]);
    let before = t.clone();
    t.reserve_read_capacity(128);
    let other = tracker(&[2], &[1]);
    assert_eq!(t.conflicts_with(&other), reference(&[2], &[2], &[1]));
    assert_eq!(t.conflicts_with(&other), before.conflicts_with(&other));
    assert_eq!(t.total_accesses(), 2);
}
#[test]
fn exhaustive_three_key_universe() {
    fn subset(mask: u8) -> Vec<u8> {
        (0..3).filter(|k| mask & (1 << k) != 0).collect()
    }
    for ar in 0..8 { for aw in 0..8 { for br in 0..8 { for bw in 0..8 {
        check(&subset(ar), &subset(aw), &subset(br), &subset(bw));
    }}}}
}
#[test]
fn equality_uses_all_256_bits() {
    let mut bytes = [0; 32];
    let zero = UInt256::from_bytes(&bytes).unwrap();
    bytes[31] = 1;
    let high = UInt256::from_bytes(&bytes).unwrap();
    let mut a = RwSetTracker::new();
    let mut b = RwSetTracker::new();
    a.record_write(zero);
    b.record_read(high);
    assert_eq!(a.conflicts_with(&b), reference(&[0], &[1], &[]));
    b.record_read(zero);
    assert_eq!(a.conflicts_with(&b), reference(&[0], &[1, 0], &[]));
}
