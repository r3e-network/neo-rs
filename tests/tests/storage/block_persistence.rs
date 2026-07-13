//! Block Persistence and Retrieval Integration Tests
//!
//! Tests for blockchain storage, indexing, and retrieval.
//!
//! NOTE: Tests removed after neo-chain crate deletion — ChainState/BlockIndexEntry
//! types (PoW concepts, cumulative difficulty, fork choice) are incompatible
//! with Neo N3 dBFT 2.0 consensus.

use std::sync::Arc;

use neo_blockchain::{
    NativePersistOptions, NativePersistResources, genesis_block,
    persist_block_natives_with_resources,
};
use neo_config::ProtocolSettings;
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Header, Witness};
use neo_storage::persistence::{StoreCache, providers::MemoryStore};

fn empty_child_block(parent: &Block, index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(parent.hash());
    header.set_timestamp(parent.header.timestamp() + 15_000);
    header.set_next_consensus(*parent.header.next_consensus());
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    Block::from_parts(header, Vec::new())
}

#[tokio::test]
async fn native_ledger_records_survive_store_cache_reopen() {
    let resources = NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));
    let settings = ProtocolSettings::default();
    let store = Arc::new(MemoryStore::new());
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(writer.data_cache().clone());

    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    let genesis_hash = genesis.try_hash().expect("genesis hash");
    persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        Arc::new(settings.clone()),
        NativePersistOptions::default(),
        &resources,
    )
    .expect("genesis persist");

    let block_one = Arc::new(empty_child_block(genesis.as_ref(), 1));
    let block_one_hash = block_one.try_hash().expect("block one hash");
    persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&block_one),
        Arc::new(settings.clone()),
        NativePersistOptions::default(),
        &resources,
    )
    .expect("block one persist");

    let ledger = LedgerContract::new();
    let before_commit = StoreCache::new_from_store(Arc::clone(&store), false);
    assert_eq!(
        ledger
            .get_block_hash(before_commit.data_cache(), 1)
            .unwrap(),
        None,
        "staged block records must not be visible through the store before commit"
    );

    writer.try_commit().expect("commit native ledger records");

    let reopened = StoreCache::new_from_store(Arc::clone(&store), false);
    let reopened_snapshot = reopened.data_cache();

    assert_eq!(ledger.current_index(reopened_snapshot).unwrap(), 1);
    assert_eq!(
        ledger.current_hash(reopened_snapshot).unwrap(),
        block_one_hash
    );
    assert_eq!(
        ledger.get_block_hash(reopened_snapshot, 0).unwrap(),
        Some(genesis_hash)
    );
    assert_eq!(
        ledger.get_block_hash(reopened_snapshot, 1).unwrap(),
        Some(block_one_hash)
    );

    let genesis_trimmed = ledger
        .get_trimmed_block(reopened_snapshot, &genesis_hash)
        .unwrap()
        .expect("genesis trimmed block");
    assert_eq!(genesis_trimmed.header.index(), 0);
    assert!(genesis_trimmed.hashes.is_empty());

    let block_one_trimmed = ledger
        .get_trimmed_block(reopened_snapshot, &block_one_hash)
        .unwrap()
        .expect("block one trimmed block");
    assert_eq!(block_one_trimmed.header.index(), 1);
    assert_eq!(*block_one_trimmed.header.prev_hash(), genesis_hash);
    assert!(block_one_trimmed.hashes.is_empty());
}
