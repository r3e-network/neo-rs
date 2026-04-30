//! Count storage entries at a specific state-root for a given native contract id.
//! Usage: count_state_at_root <state_root_db_path> <root_hash_hex> <contract_id> [prefix_byte]
#![cfg(feature = "rocksdb")]

use neo_core::persistence::providers::RocksDBStoreProvider;
use neo_core::persistence::{i_store_provider::IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::UInt256;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("missing state root db path")?;
    let root_hex = args.next().ok_or("missing root hash hex")?;
    let id: i32 = args.next().ok_or("missing contract id")?.parse()?;
    let prefix_byte: Option<u8> = args.next().map(|s| {
        u8::from_str_radix(s.strip_prefix("0x").unwrap_or(&s), 16).expect("hex prefix")
    });

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store));
    let state_store = StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: path.clone(),
            ..Default::default()
        },
    );

    let root = UInt256::parse(root_hex.strip_prefix("0x").unwrap_or(&root_hex))?;
    let mut trie = state_store.trie_for_root(root);

    // StorageKey layout: 4-byte LE contract id + arbitrary key bytes.
    let id_le = id.to_le_bytes();
    let prefix = if let Some(b) = prefix_byte {
        vec![id_le[0], id_le[1], id_le[2], id_le[3], b]
    } else {
        vec![id_le[0], id_le[1], id_le[2], id_le[3]]
    };

    let mut count = 0u64;
    let mut by_prefix = std::collections::BTreeMap::<u8, u64>::new();
    let mut sample = Vec::<(Vec<u8>, Vec<u8>)>::new();

    let entries = trie.find(&prefix, None)?;
    for entry in entries {
        let k = &entry.key;
        let v = &entry.value;
        count += 1;
        if k.len() > 4 {
            *by_prefix.entry(k[4]).or_insert(0) += 1;
        }
        if sample.len() < 10 {
            sample.push((k.clone(), v.clone()));
        }
    }

    println!("root={} id={} total_entries={}", root, id, count);
    for (prefix_byte, c) in &by_prefix {
        println!("  prefix=0x{:02x} count={}", prefix_byte, c);
    }
    println!("first {} entries:", sample.len());
    for (k, v) in &sample {
        println!("  key={} val={} ({}B)", hex::encode(k), hex::encode(&v[..v.len().min(48)]), v.len());
    }

    Ok(())
}
