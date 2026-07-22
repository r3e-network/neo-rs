use super::*;

#[test]
fn record_tip_tracks_highest_index() {
    let ledger = LedgerContext::default();
    ledger.record_tip(7);
    ledger.record_tip(5);
    ledger.record_tip(12);
    assert_eq!(ledger.current_height(), 12);
}

#[test]
fn block_hash_at_unknown_index_returns_none() {
    let ledger = LedgerContext::default();
    assert!(ledger.block_hash_at(0).is_none());
    assert!(ledger.block_hash_at(123).is_none());
}

#[test]
fn insert_shared_block_records_body_hash_and_tip_without_call_site_clone() {
    let ledger = LedgerContext::default();
    let mut header = Header::new();
    header.set_index(7);
    header.set_nonce(42);
    let block = Arc::new(Block::from_parts(header, vec![]));
    let expected_hash = block.try_hash().expect("hash block");

    let inserted_hash = ledger
        .insert_block_arc(Arc::clone(&block))
        .expect("insert shared block");

    assert_eq!(inserted_hash, expected_hash);
    assert_eq!(ledger.current_height(), 7);
    assert_eq!(ledger.block_hash_at(7), Some(expected_hash));
    assert_eq!(
        ledger
            .get_block(&expected_hash)
            .expect("cached block body")
            .index(),
        7
    );
}

#[test]
fn block_body_cache_is_bounded_and_evicts_oldest() {
    // Capacity of 2: inserting 3 blocks must evict the first body, but the
    // height->hash index must still record all three (it is kept full).
    let ledger = LedgerContext::with_capacity(2);

    let mut hashes = Vec::new();
    for i in 0..3u32 {
        let mut header = Header::new();
        header.set_index(i);
        // distinct nonce keeps each header hash unique
        header.set_nonce(1000 + i as u64);
        let block = Block::from_parts(header, vec![]);
        let hash = ledger.insert_block(block).expect("insert");
        hashes.push(hash);
    }

    // Oldest body (index 0) was evicted from the bounded in-memory cache...
    assert!(
        ledger.get_block(&hashes[0]).is_none(),
        "block body cache must evict beyond capacity"
    );
    // ...but the two most-recent bodies are still resident.
    assert!(ledger.get_block(&hashes[1]).is_some());
    assert!(ledger.get_block(&hashes[2]).is_some());

    // The cheap height->hash index is NOT evicted: every height resolves.
    assert_eq!(ledger.block_hash_at(0), Some(hashes[0]));
    assert_eq!(ledger.block_hash_at(1), Some(hashes[1]));
    assert_eq!(ledger.block_hash_at(2), Some(hashes[2]));
    assert_eq!(ledger.current_height(), 2);
}
