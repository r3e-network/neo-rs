use super::*;
use neo_primitives::UInt256;

fn dummy_header(index: u32) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header.set_timestamp(index as u64);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_next_consensus(neo_primitives::UInt160::zero());
    header
}

#[test]
fn empty_cache_has_no_last() {
    let cache = HeaderCache::new();
    assert_eq!(cache.count(), 0);
    assert!(cache.last().is_none());
}

#[test]
fn max_headers_matches_csharp() {
    assert_eq!(MAX_HEADERS, 10_000);
}

#[test]
fn add_appends_to_tail() {
    let cache = HeaderCache::new();
    cache.add(dummy_header(7));
    cache.add(dummy_header(8));
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.last().unwrap().index(), 8);
}

#[test]
fn get_returns_matching_header() {
    let cache = HeaderCache::new();
    cache.add(dummy_header(5));
    cache.add(dummy_header(6));
    assert_eq!(cache.get(5).unwrap().index(), 5);
    assert_eq!(cache.get(6).unwrap().index(), 6);
    assert!(cache.get(7).is_none());
}

#[test]
fn remove_up_to_drops_lower_indices() {
    let cache = HeaderCache::new();
    for i in 0..5 {
        cache.add(dummy_header(i));
    }
    let removed = cache.remove_up_to(2);
    assert_eq!(removed, 3);
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.first_index().unwrap(), 3);
}
