use neo_core::persistence::{providers::RocksDBStoreProvider, IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateStoreSnapshot,
};
use neo_crypto::mpt_trie::Trie;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "data/Plugins/StateService/Data_MPT_334F454E".to_string());
    let target_index: u32 = args
        .next()
        .unwrap_or_else(|| "0".to_string())
        .parse()?;

    let config = StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;

    // Read state root at target index: key = [0x01, index as BE u32]
    let mut root_key = vec![0x01];
    root_key.extend_from_slice(&target_index.to_be_bytes());
    let snapshot = store.get_snapshot();
    let root_data = snapshot
        .try_get(&root_key)
        .ok_or_else(|| format!("no state root at index {target_index}"))?;

    // Parse root hash from StateRoot: [version u8][index u32 LE][root UInt256 LE]
    if root_data.len() < 37 {
        return Err(format!("state root data too short: {} bytes", root_data.len()).into());
    }
    let root_hash_bytes: [u8; 32] = root_data[5..37].try_into()?;
    let root_hash = neo_primitives::UInt256::from_bytes(&root_hash_bytes)?;
    println!(
        "State root at index {}: 0x{}",
        target_index,
        hex::encode(root_hash.to_bytes())
    );

    // Build trie
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store));
    let ss = StateStoreSnapshot::new(backend);
    let mut trie = Trie::new(Arc::new(ss), Some(root_hash), true);

    // Dump all entries
    let entries = trie.find(&[], None)?;
    println!("Total entries: {}", entries.len());
    for entry in &entries {
        let key_hex = hex::encode(&entry.key);
        let value_hex = hex::encode(&entry.value);
        println!("key=0x{key_hex} value=0x{value_hex}");
    }

    Ok(())
}
