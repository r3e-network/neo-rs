#![cfg(feature = "rocksdb")]

//! Contiguous structural integrity check over the locally stored MainNet chain.
//!
//! For every height it loads the block, re-computes its hash, checks the
//! index/previous-hash link, and re-computes the Merkle root from the block's
//! transactions. This is not a full execution replay, but it verifies that the
//! stored chain itself is contiguous and internally consistent at every height.
//!
//! Run with:
//!   NEO_CHAIN_DB_PATH=data/mainnet-protocol-consistency \
//!     cargo test -p neo-core --features rocksdb --test mainnet_chain_integrity -- --ignored --nocapture

use neo_core::UInt256;
use neo_core::persistence::data_cache::DataCacheConfig;
use neo_core::persistence::{
    StorageConfig, StoreCache, StoreProvider, providers::RocksDBStoreProvider,
};
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::validation::validate_merkle_root;
use std::path::Path;

#[test]
#[ignore = "requires a local MainNet chain database"]
fn stored_mainnet_chain_is_contiguous_and_has_valid_merkle_roots() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let db_path = std::env::var("NEO_CHAIN_DB_PATH")
        .ok()
        .filter(|path| !path.is_empty())
        .map(|path| Path::new(&path).to_path_buf())
        .unwrap_or_else(|| repo_root.join("data/mainnet-protocol-consistency"));
    if !db_path.exists() {
        eprintln!("[SKIPPED] {} does not exist", db_path.display());
        return;
    }

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path,
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("").expect("open chain store");
    let cache = StoreCache::new_from_snapshot_with_config(
        store.snapshot(),
        DataCacheConfig {
            max_entries: usize::MAX,
            track_reads_in_write_cache: false,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        },
    );
    let ledger = LedgerContract::new();

    let height = ledger.current_index(&cache).expect("current height");
    let max_height = std::env::var("NEO_CHAIN_MAX_HEIGHT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(height)
        .min(height);

    let mut prev_hash = UInt256::zero();
    let mut total_txs = 0u64;
    for index in 0..=max_height {
        let block = ledger
            .get_block(&cache, HashOrIndex::Index(index))
            .expect("read block")
            .unwrap_or_else(|| panic!("missing block at height {index}"));
        assert_eq!(block.index(), index, "block index mismatch");
        let stored_hash = block.hash();
        if index > 0 {
            assert_eq!(
                block.header.previous_hash, prev_hash,
                "prev-hash link broken at {index}"
            );
        }
        validate_merkle_root(&block.header.merkle_root, &block.transactions)
            .unwrap_or_else(|error| panic!("merkle root invalid at {index}: {error}"));
        prev_hash = stored_hash;
        total_txs += block.transactions.len() as u64;
    }

    eprintln!(
        "stored_chain_ok heights=0..={max_height} blocks={} transactions={total_txs}",
        max_height + 1
    );
}
