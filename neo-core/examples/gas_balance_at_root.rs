use neo_core::persistence::{providers::RocksDBStoreProvider, StoreProvider, StorageConfig};
use neo_core::smart_contract::StorageKey;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::{UInt160, UInt256};
use std::path::PathBuf;
use std::sync::Arc;

// Read GAS balance for an account at a specific historical state root.
// Usage:
//   cargo run --release -p neo-core --features rocksdb --example gas_balance_at_root \
//     -- /home/neo/git/neo-rs/data/mainnet/StateRoot 1283520 0x673a663cebe612f6e63e9bf85a2076d601fe5fb9
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: gas_balance_at_root <stateroot_db_path> <height> <account_hash>")?;
    let height: u32 = args.next().ok_or("missing height")?.parse()?;
    let account_raw = args.next().ok_or("missing account")?;
    let account = UInt160::parse(account_raw.trim_start_matches("0x"))?;

    let config = StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let state_db = provider.get_store("")?;
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    let store = StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: path.clone(),
            ..StateServiceSettings::default()
        },
    );

    let Some(root_record) = store.get_state_root(height) else {
        println!("height={} root=ABSENT", height);
        return Ok(());
    };
    let root: UInt256 = root_record.root_hash;
    println!("height={} root={}", height, root);

    let mut trie = store.trie_for_root(root);
    let key = StorageKey::create_with_uint160(-6, 20, &account);
    let key_bytes = key.to_array();
    match trie.get(&key_bytes)? {
        Some(value) => {
            println!(
                "account={} key=0x{} value=0x{}",
                account,
                hex::encode(&key_bytes),
                hex::encode(&value)
            );
        }
        None => {
            println!(
                "account={} key=0x{} value=<missing>",
                account,
                hex::encode(&key_bytes)
            );
        }
    }
    Ok(())
}
