#[cfg(feature = "rocksdb")]
use neo_core::persistence::{StorageConfig, StoreCache, StoreProvider, providers::RocksDBStoreProvider};
#[cfg(feature = "rocksdb")]
use neo_core::smart_contract::native::ContractManagement;
#[cfg(feature = "rocksdb")]
use std::path::PathBuf;

#[cfg(feature = "rocksdb")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: inspect_contract_by_id <db_path> <id>")?;
    let id: i32 = args.next().ok_or("missing id")?.parse()?;
    let config = StorageConfig { path: PathBuf::from(path), read_only: true, ..Default::default() };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let cache = StoreCache::new_from_snapshot(store.snapshot());
    match ContractManagement::get_contract_by_id_from_store_cache(&cache, id)? {
        Some(contract) => {
            println!(
                "id={} update={} manifest={}",
                contract.id,
                contract.update_counter,
                contract.manifest.name
            );
            println!("script={}", hex::encode(&contract.nef.script));
        }
        None => println!("no contract with id {id}"),
    }
    Ok(())
}

#[cfg(not(feature = "rocksdb"))]
fn main() {}
