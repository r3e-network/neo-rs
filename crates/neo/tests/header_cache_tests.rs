use neo_core::ledger::header_cache::{HeaderCache, MAX_HEADERS};
use neo_core::network::p2p::payloads::header::Header;

fn make_header(index: u32) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header
}

#[test]
fn header_cache_drops_oldest_when_full() {
    let cache = HeaderCache::new();
    for index in 0..=(MAX_HEADERS as u32) {
        assert!(cache.add(make_header(index)));
    }

    assert_eq!(cache.count(), MAX_HEADERS);
    assert_eq!(cache.first_index(), Some(1));
    assert!(cache.get(0).is_none());
    assert!(cache.get(1).is_some());
}

#[test]
fn header_cache_rejects_non_increasing_indexes() {
    let cache = HeaderCache::new();
    assert!(cache.add(make_header(10)));
    assert!(!cache.add(make_header(10)));
    assert!(!cache.add(make_header(5)));
}

#[test]
fn header_cache_remove_up_to_clears_consumed_headers() {
    let cache = HeaderCache::new();
    for index in 0..5 {
        cache.add(make_header(index));
    }

    let removed = cache.remove_up_to(2);
    assert_eq!(removed, 3);
    assert_eq!(cache.first_index(), Some(3));
    assert!(cache.get(2).is_none());
}
