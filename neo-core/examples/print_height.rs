use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::LedgerContract;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "./data/testnet".to_string());
    let probe_index = args.next().map(|v| v.parse::<u32>()).transpose()?;

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    let ledger = LedgerContract::new();
    let height = ledger.current_index(&cache)?;
    println!("current_index={height}");
    if let Some(index) = probe_index {
        match ledger.get_block_hash_by_index(&cache, index)? {
            Some(hash) => println!("block_hash[{index}]={hash}"),
            None => println!("block_hash[{index}]=<none>"),
        }
    }
    Ok(())
}
