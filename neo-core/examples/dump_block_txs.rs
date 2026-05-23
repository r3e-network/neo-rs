use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::LedgerContract;
use std::path::PathBuf;

// Dump all transactions in a given block: hash, sender, vm_state, gas_consumed.
// Helps identify which txs FAULTed where they should have HALTed.
//
// Usage:
//   cargo run --release -p neo-core --features rocksdb \
//     --example dump_block_txs -- /home/neo/git/neo-rs/data/mainnet 1283521
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_block_txs <db_path> <block_index>")?;
    let block_idx: u32 = args
        .next()
        .ok_or("usage: dump_block_txs <db_path> <block_index>")?
        .parse()?;

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

    let block = ledger
        .get_block(&cache, HashOrIndex::Index(block_idx))?
        .ok_or("block not found")?;
    println!(
        "block_index={} hash=0x{}",
        block_idx,
        hex::encode(block.hash().as_bytes())
    );
    println!("tx_count={}", block.transactions.len());

    for (i, tx) in block.transactions.iter().enumerate() {
        let hash = tx.hash();
        let sender = tx.sender().unwrap_or_default();
        let sysfee = tx.system_fee();
        let netfee = tx.network_fee();
        // Get persisted tx state for vm_state.
        let state = ledger.get_transaction_state(&cache, &hash)?;
        let (vm_state, raw_state) = if let Some(s) = state {
            (format!("{:?}", s.vm_state()), s.vm_state_raw())
        } else {
            ("<none>".into(), 0)
        };
        println!(
            "  tx[{}] hash=0x{} sender={} sysfee={} netfee={} vm_state={} raw_state={}",
            i,
            hex::encode(hash.as_bytes()),
            sender,
            sysfee,
            netfee,
            vm_state,
            raw_state
        );
    }
    Ok(())
}
