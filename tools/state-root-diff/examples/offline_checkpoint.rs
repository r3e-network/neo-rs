use anyhow::{Context, Result};
use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
use neo_storage::persistence::providers::rocksdb::RocksDBStoreProvider;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::store_cache::StoreCache;
use neo_storage::persistence::store_provider::StoreProvider;
use neo_system::state_service::commit_handlers::read_state_root_record;
use std::path::PathBuf;

fn main() -> Result<()> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: offline_checkpoint <store-path>"),
    );
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: path.clone(),
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let cache = StoreCache::new(store);
    let ledger = LedgerContract::new();
    let height = ledger.current_index(&cache)?;
    let block = ledger
        .get_block(&cache, HashOrIndex::Index(height))?
        .context("checkpoint block is missing")?;
    let root = read_state_root_record(&cache, height)
        .map_err(anyhow::Error::msg)?
        .context("checkpoint state root is missing")?;
    println!("height={height}");
    println!("block_hash={}", block.hash());
    println!("state_root={}", root.root_hash);
    Ok(())
}
