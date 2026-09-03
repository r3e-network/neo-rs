use neo_core::ledger::header_cache::{HeaderCache, MAX_HEADERS};

use neo_core::network::p2p::payloads::Header;

fn header_with_index(index: u32) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header
}

#[test]
fn header_cache_basic_behaviour() {
    let cache = HeaderCache::new();

    let header = header_with_index(1);
    assert!(cache.add(header.clone()));

    let stored = cache.get(1).expect("header should exist");
    assert_eq!(stored.index(), 1);

    assert_eq!(cache.count(), 1);
    assert!(!cache.full());
    assert_eq!(cache.last().unwrap().index(), 1);
    assert!(cache.get(2).is_none());

    let mut iter = cache.iter();
    assert_eq!(iter.next().unwrap().index(), 1);
    assert!(iter.next().is_none());

    let removed = cache.try_remove_first().expect("header should be removed");
    assert_eq!(removed.index(), 1);

    assert_eq!(cache.count(), 0);
    assert!(!cache.full());
    assert!(cache.last().is_none());
    assert!(cache.get(1).is_none());
}

#[test]
fn header_cache_respects_capacity() {
    let cache = HeaderCache::new();

    for index in 0..MAX_HEADERS {
        assert!(cache.add(header_with_index(index as u32)));
    }

    assert_eq!(cache.count(), MAX_HEADERS);
    assert!(cache.full());
    assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);

    // Adding beyond the limit is rejected when the cache is full.
    assert!(!cache.add(header_with_index(MAX_HEADERS as u32)));
    assert_eq!(cache.count(), MAX_HEADERS);
    assert_eq!(cache.first_index().unwrap(), 0);
    assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);
    assert!(cache.get(MAX_HEADERS as u32).is_none());
}
