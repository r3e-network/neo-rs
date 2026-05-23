#[cfg(feature = "rocksdb")]
use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StorageKey, StoreCache,
};
#[cfg(feature = "rocksdb")]
use neo_core::UInt160;
#[cfg(feature = "rocksdb")]
use std::path::PathBuf;

// Print the GAS balance for an account from a read-only mainnet DB snapshot.
// Usage:
//   cargo run --release -p neo-core --example inspect_gas_balance \
//     -- /home/neo/git/neo-rs/data/mainnet 0xe69f64c8fa57c7b23a2c75f4b234c030993dc39b
#[cfg(feature = "rocksdb")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: inspect_gas_balance <db_path> <account_hash>")?;
    let account_raw = args
        .next()
        .ok_or("usage: inspect_gas_balance <db_path> <account_hash>")?;
    let account = UInt160::parse(account_raw.trim_start_matches("0x"))?;

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    // GasToken native contract: id=-6, account-prefix=20.
    let key = StorageKey::create_with_uint160(-6, 20, &account);
    println!("key_hex=0x{}", hex::encode(key.to_array()));
    let Some(item) = cache.get(&key) else {
        println!("gas_balance=<missing> (account never received GAS in this state)");
        return Ok(());
    };
    let raw = item.get_value();
    println!("raw_hex=0x{}", hex::encode(&raw));
    let bal = item.to_bigint();
    println!("balance_datoshi={}", bal);
    Ok(())
}

#[cfg(not(feature = "rocksdb"))]
fn main() {
    eprintln!("inspect_gas_balance requires the neo-core `rocksdb` feature.");
}
