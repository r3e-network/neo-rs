use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::{GasToken, NeoToken};
use neo_core::UInt160;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: check_unclaimed_gas <db_path> <account_hash> [end]")?;
    let account_raw = args
        .next()
        .ok_or("usage: check_unclaimed_gas <db_path> <account_hash> [end]")?;
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

    let ledger = neo_core::smart_contract::native::LedgerContract::new();
    let current = ledger.current_index(&cache)?;
    let end = match args.next() {
        Some(raw) => raw.parse::<u32>()?,
        None => current.saturating_add(1),
    };

    let neo = NeoToken::new();
    let gas = GasToken::new();
    let neo_balance = neo.balance_of_snapshot(&cache, &account)?;
    let gas_balance = gas.balance_of_snapshot(&cache, &account);
    let unclaimed = neo.unclaimed_gas(&cache, &account, end)?;

    println!("current_index={current}");
    println!("account={account}");
    println!("neo_balance={neo_balance}");
    println!("gas_balance={gas_balance}");
    println!("end={end}");
    println!("unclaimed_gas={unclaimed}");

    Ok(())
}
