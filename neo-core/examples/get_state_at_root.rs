use neo_core::persistence::{providers::RocksDBStoreProvider, IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{SnapshotBackedStateStoreBackend, StateStore};
use std::path::PathBuf;

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    hex::decode(trimmed).map_err(|err| format!("invalid hex `{input}`: {err}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("missing state root path")?;
    let index: u32 = args.next().ok_or("missing state root index")?.parse()?;
    let key_hex = args.next().ok_or("missing trie key hex")?;
    let key = decode_hex(&key_hex)?;

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let backend = std::sync::Arc::new(SnapshotBackedStateStoreBackend::new(store));
    let state_store = StateStore::new(
        backend,
        neo_core::state_service::state_store::StateServiceSettings {
            full_state: true,
            path,
            ..Default::default()
        },
    );

    let root = state_store
        .get_state_root(index)
        .ok_or("missing state root")?
        .root_hash;
    let mut trie = state_store.trie_for_root(root);
    let value = trie.get(&key)?;
    match value {
        Some(bytes) => {
            println!("{}", hex::encode(bytes));
        }
        None => {
            println!("<missing>");
        }
    }
    Ok(())
}
