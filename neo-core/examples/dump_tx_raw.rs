use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::UInt256;
use std::path::PathBuf;

// Print the script bytes of a tx as hex.
//   cargo run --release -p neo-core --features rocksdb --example dump_tx_raw \
//     -- /home/neo/git/neo-rs/data/mainnet 0xd52a392671690cf4e824e52bdcc03261fec63d7665cc87ba108233debe028c74
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_tx_raw <db_path> <tx_hash>")?;
    let tx_hash_raw = args
        .next()
        .ok_or("usage: dump_tx_raw <db_path> <tx_hash>")?;
    let tx_hash = UInt256::parse(tx_hash_raw.trim_start_matches("0x"))?;

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

    let tx_state = ledger
        .get_transaction_state(&cache, &tx_hash)?
        .ok_or("transaction not found")?;
    let tx = tx_state.transaction();
    println!("block={}", tx_state.block_index());
    println!("vm_state={:?} raw_state={}", tx_state.vm_state(), tx_state.vm_state_raw());
    println!("sender={}", tx.sender().map(|s| s.to_string()).unwrap_or_default());
    println!("sysfee={} netfee={}", tx.system_fee(), tx.network_fee());
    println!("script_hex={}", hex::encode(tx.script()));
    println!("script_len={}", tx.script().len());
    Ok(())
}
