use super::*;

fn parked_block(index: u32) -> UnverifiedBlock {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    UnverifiedBlock::new(
        Arc::new(neo_payloads::Block::from_parts(header, vec![])),
        false,
        false,
    )
}

#[test]
fn classify_already_seen_for_past_height() {
    assert_eq!(
        ImportDisposition::classify_import_block(10, 5),
        ImportDisposition::AlreadySeen
    );
    assert_eq!(
        ImportDisposition::classify_import_block(10, 10),
        ImportDisposition::AlreadySeen
    );
}

#[test]
fn classify_next_expected_when_in_sequence() {
    assert_eq!(
        ImportDisposition::classify_import_block(7, 8),
        ImportDisposition::NextExpected
    );
}

#[test]
fn classify_future_gap_for_skip() {
    assert_eq!(
        ImportDisposition::classify_import_block(3, 8),
        ImportDisposition::FutureGap
    );
}

#[test]
fn schedule_idle_only_when_more_pending_without_backlog() {
    assert!(should_schedule_reverify_idle(true, false));
    assert!(!should_schedule_reverify_idle(false, false));
    assert!(!should_schedule_reverify_idle(true, true));
}

#[test]
fn unverified_cache_evicts_highest_heights_first() {
    let mut cache = UnverifiedBlockCache::default();
    cache.push(parked_block(10));
    cache.push(parked_block(20));
    cache.push(parked_block(20));
    cache.push(parked_block(20));

    assert_eq!(cache.evict_highest(2), 2);
    assert_eq!(cache.len(), 2);
    assert_eq!(
        cache.pop_front(20).map(|block| block.block.index()),
        Some(20)
    );
    assert_eq!(
        cache.pop_front(10).map(|block| block.block.index()),
        Some(10)
    );
    assert_eq!(cache.len(), 0);
}

#[test]
fn unverified_cache_remove_up_to_keeps_count_exact() {
    let mut cache = UnverifiedBlockCache::default();
    cache.push(parked_block(10));
    cache.push(parked_block(11));
    cache.push(parked_block(12));

    assert_eq!(cache.remove_up_to(11), 2);
    assert_eq!(cache.len(), 1);
    assert!(cache.pop_front(11).is_none());
    assert_eq!(
        cache.pop_front(12).map(|block| block.block.index()),
        Some(12)
    );
    assert_eq!(cache.len(), 0);
}
