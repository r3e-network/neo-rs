use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::LedgerContract;
use neo_core::UInt256;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "./data/testnet".to_string());
    let tx_hash = args.next().ok_or("missing tx hash argument")?;
    let hash = UInt256::parse(tx_hash.trim_start_matches("0x"))?;

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

    match ledger.get_transaction_state(&cache, &hash)? {
        Some(state) => {
            println!("block_index={}", state.block_index());
            println!("vm_state={:?} raw={}", state.vm_state(), state.vm_state_raw());
            let tx = state.transaction();
            println!("sender={}", tx.sender().unwrap_or_default());
            println!("sys_fee={} net_fee={}", tx.system_fee(), tx.network_fee());
            for (idx, signer) in tx.signers().iter().enumerate() {
                println!(
                    "signer[{idx}].account={} signer[{idx}].scopes={}",
                    signer.account, signer.scopes
                );
            }
            for (idx, attr) in tx.attributes().iter().enumerate() {
                println!("attribute[{idx}]={attr:?}");
            }
        }
        None => println!("tx_state=<none>"),
    }

    Ok(())
}
