use super::*;
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_primitives::{UInt160, WitnessScope};

fn make_signed_transaction() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_valid_until_block(10);
    tx.add_signer(Signer::new(
        UInt160::default(),
        WitnessScope::CALLED_BY_ENTRY,
    ));
    tx.add_witness(Witness::new());
    tx
}

#[test]
fn record_tip_tracks_highest_index() {
    let ledger = LedgerContext::default();
    ledger.record_tip(7);
    ledger.record_tip(5);
    ledger.record_tip(12);
    assert_eq!(ledger.current_height(), 12);
}

#[test]
fn insert_and_get_transaction() {
    let ledger = LedgerContext::default();
    let tx = make_signed_transaction();
    let hash = tx.hash();
    ledger.insert_transaction(tx).expect("insert");
    assert!(ledger.get_transaction(&hash).is_some());
}

#[test]
fn block_hash_at_unknown_index_returns_none() {
    let ledger = LedgerContext::default();
    assert!(ledger.block_hash_at(0).is_none());
    assert!(ledger.block_hash_at(123).is_none());
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
