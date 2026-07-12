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
fn hash_at_returns_matching_hash_for_contiguous_cache() {
    let cache = HeaderCache::new();
    let header5 = dummy_header(5);
    let header6 = dummy_header(6);
    let hash5 = header5.hash();
    let hash6 = header6.hash();

    cache.add(header5);
    cache.add(header6);

    assert_eq!(cache.hash_at(5), Some(hash5));
    assert_eq!(cache.hash_at(6), Some(hash6));
    assert_eq!(cache.hash_at(7), None);
}

#[test]
fn get_and_hash_at_fall_back_for_noncontiguous_test_inserts() {
    let cache = HeaderCache::new();
    let header5 = dummy_header(5);
    let header7 = dummy_header(7);
    let hash7 = header7.hash();

    cache.add(header5);
    cache.add(header7);

    assert_eq!(cache.get(7).unwrap().index(), 7);
    assert_eq!(cache.hash_at(7), Some(hash7));
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

#[test]
fn hash_at_tracks_front_offset_after_prune() {
    let cache = HeaderCache::new();
    let header3 = dummy_header(3);
    let header4 = dummy_header(4);
    let hash4 = header4.hash();

    cache.add(dummy_header(2));
    cache.add(header3);
    cache.add(header4);
    assert_eq!(cache.remove_up_to(2), 1);

    assert_eq!(cache.first_index(), Some(3));
    assert_eq!(cache.hash_at(4), Some(hash4));
}

#[test]
fn clear_discards_the_entire_ahead_view() {
    let cache = HeaderCache::new();
    cache.add(dummy_header(8));
    cache.add(dummy_header(9));

    assert_eq!(cache.clear(), 2);
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.hash_at(8), None);
    assert_eq!(cache.clear(), 0, "clear stays idempotent");
}
