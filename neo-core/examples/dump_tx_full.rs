use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::UInt256;
use std::path::PathBuf;

// Dump all tx fields in a copy-pasteable form for use in a reproducer.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_tx_full <db_path> <tx_hash>")?;
    let tx_hash_raw = args
        .next()
        .ok_or("usage: dump_tx_full <db_path> <tx_hash>")?;
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
        .ok_or("tx not found")?;
    let tx = tx_state.transaction();

    println!("// tx={}", tx_hash);
    println!("// block_index={}", tx_state.block_index());
    println!("version={}", tx.version());
    println!("nonce={}", tx.nonce());
    println!("system_fee={}", tx.system_fee());
    println!("network_fee={}", tx.network_fee());
    println!("valid_until_block={}", tx.valid_until_block());
    println!("script_b64={}", BASE64.encode(tx.script()));
    println!("script_hex=0x{}", hex::encode(tx.script()));
    println!("signers=[");
    for s in tx.signers() {
        println!(
            "    Signer {{ account={}, scopes={:?}, allowed_contracts={:?}, allowed_groups_count={}, rules_count={} }},",
            s.account, s.scopes,
            s.allowed_contracts.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
            s.allowed_groups.len(),
            s.rules.len()
        );
    }
    println!("]");
    println!("attributes_count={}", tx.attributes().len());
    println!("witnesses=[");
    for w in tx.witnesses() {
        println!(
            "    Witness {{ inv_b64={}, ver_b64={} }},",
            BASE64.encode(w.invocation_script()),
            BASE64.encode(w.verification_script())
        );
    }
    println!("]");
    Ok(())
}
