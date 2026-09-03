use neo_core::ledger::header_cache::{HeaderCache, MAX_HEADERS};
use neo_core::network::p2p::payloads::header::Header;

fn make_header(index: u32) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header
}

#[test]
fn header_cache_basics_match_csharp() {
    let cache = HeaderCache::new();
    let header = make_header(1);
    assert!(cache.add(header.clone()));

    let got = cache.get(1).expect("header at index");
    assert_eq!(got.index(), 1);
    assert_eq!(cache.count(), 1);
    assert!(!cache.full());
    assert_eq!(cache.last().unwrap().index(), 1);
    assert!(cache.get(2).is_none());

    let mut iter = cache.iter();
    assert_eq!(iter.next().unwrap().index(), 1);
    assert!(iter.next().is_none());

    let removed = cache.try_remove_first();
    assert!(removed.is_some());
    assert_eq!(cache.count(), 0);
    assert!(!cache.full());
    assert!(cache.last().is_none());
    assert!(cache.get(1).is_none());
}

#[test]
fn header_cache_refuses_add_when_full() {
    let cache = HeaderCache::new();
    for index in 0..(MAX_HEADERS as u32) {
        assert!(cache.add(make_header(index)));
    }

    assert!(cache.full());
    assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);
    assert!(!cache.add(make_header(MAX_HEADERS as u32)));
    assert_eq!(cache.count(), MAX_HEADERS);
    assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);
    assert!(cache.get(MAX_HEADERS as u32).is_none());
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
