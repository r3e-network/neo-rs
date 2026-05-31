use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::persistence::{
    providers::RocksDBStoreProvider, StoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::{HashOrIndex, LedgerContract};
use std::path::PathBuf;

// Dump block header fields for use in a reproducer.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_block_header <db_path> <index>")?;
    let block_idx: u32 = args
        .next()
        .ok_or("usage: dump_block_header <db_path> <index>")?
        .parse()?;
    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    let ledger = LedgerContract::new();
    let block = ledger
        .get_block(&cache, HashOrIndex::Index(block_idx))?
        .ok_or("block not found")?;
    let h = &block.header;
    println!("version={}", h.version());
    println!("prev_hash=0x{}", hex::encode(h.prev_hash().as_bytes()));
    println!("merkle_root=0x{}", hex::encode(h.merkle_root().as_bytes()));
    println!("timestamp={}", h.timestamp());
    println!("nonce={}", h.nonce());
    println!("index={}", h.index());
    println!("primary_index={}", h.primary_index());
    println!("next_consensus={}", h.next_consensus());
    if let Some(w) = [&h.witness].first() {
        println!("witness_inv_b64={}", BASE64.encode(w.invocation_script()));
        println!("witness_ver_b64={}", BASE64.encode(w.verification_script()));
    }
    Ok(())
}
