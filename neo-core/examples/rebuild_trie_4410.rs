use neo_core::persistence::{providers::RocksDBStoreProvider, IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{SnapshotBackedStateStoreBackend, StateStoreSnapshot};
use neo_crypto::mpt_trie::{Trie, TrieEntry};
use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Minimal in-memory store for trie rebuilding that avoids RocksDB conflicts.
struct InMemoryMptStore {
    data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
}

impl InMemoryMptStore {
    fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}

impl neo_crypto::mpt_trie::MptStoreSnapshot for InMemoryMptStore {
    fn try_get(&self, key: &[u8]) -> neo_crypto::mpt_trie::MptResult<Option<Vec<u8>>> {
        Ok(self.data.read().get(key).cloned())
    }
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.write().insert(key, value);
        Ok(())
    }
    fn delete(&self, key: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.write().remove(&key);
        Ok(())
    }
}

fn build_trie_from_entries(entries: &[TrieEntry]) -> UInt256 {
    let store = Arc::new(InMemoryMptStore::new());
    let mut trie = Trie::new(store, None, true);

    for entry in entries {
        trie.put(&entry.key, &entry.value).expect("put failed");
    }

    trie.root_hash().unwrap_or_else(UInt256::zero)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/Plugins/StateService/Data_MPT_334F454E".to_string());
    let target_index: u32 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "4".to_string())
        .parse()?;

    let config = StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;

    // Read state root at target index
    let mut root_key = vec![0x01];
    root_key.extend_from_slice(&target_index.to_be_bytes());
    let snapshot = store.get_snapshot();
    let root_data = snapshot
        .try_get(&root_key)
        .ok_or_else(|| format!("no state root at index {target_index}"))?;

    if root_data.len() < 37 {
        return Err(format!("state root data too short: {} bytes", root_data.len()).into());
    }
    let root_hash_bytes: [u8; 32] = root_data[5..37].try_into()?;
    let stored_root_hash = UInt256::from_bytes(&root_hash_bytes)?;
    println!(
        "Stored state root at index {target_index}: 0x{}",
        hex::encode(stored_root_hash.to_bytes())
    );

    // Build trie from existing data and dump all entries
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store));
    let ss = StateStoreSnapshot::new(backend);
    let mut existing_trie = Trie::new(Arc::new(ss), Some(stored_root_hash), true);

    let entries = existing_trie.find(&[], None)?;
    println!("Total entries in existing trie: {}", entries.len());
    println!();

    // Rebuild in forward order (in-memory)
    let fwd_root = build_trie_from_entries(&entries);
    println!(
        "Forward-order rebuilt root: 0x{}",
        hex::encode(fwd_root.to_bytes())
    );
    if fwd_root == stored_root_hash {
        println!("  => MATCHES stored root");
    } else {
        println!("  => MISMATCH with stored root!");
    }

    // Rebuild in reverse order
    let mut rev_entries = entries.clone();
    rev_entries.reverse();
    let rev_root = build_trie_from_entries(&rev_entries);
    println!(
        "Reverse-order rebuilt root: 0x{}",
        hex::encode(rev_root.to_bytes())
    );
    if rev_root == stored_root_hash {
        println!("  => MATCHES stored root");
    } else if rev_root == fwd_root {
        println!("  => same as forward order");
    } else {
        println!("  => DIFFERENT from both stored and forward!");
    }

    // Rebuild in sorted order
    let mut sorted_entries = entries.clone();
    sorted_entries.sort_by(|a, b| a.key.cmp(&b.key));
    let sorted_root = build_trie_from_entries(&sorted_entries);
    println!(
        "Sorted-order rebuilt root:   0x{}",
        hex::encode(sorted_root.to_bytes())
    );
    if sorted_root == stored_root_hash {
        println!("  => MATCHES stored root");
    } else if sorted_root == fwd_root {
        println!("  => same as forward order");
    } else {
        println!("  => DIFFERENT from both stored and forward!");
    }

    // Analysis
    println!();
    println!("--- Analysis ---");
    if fwd_root == rev_root && rev_root == sorted_root {
        println!("All insertion orders produce the same root hash.");
        println!("The trie algorithm is order-independent (correct for MPT).");
        if fwd_root != stored_root_hash {
            println!("BUT the stored root differs - persisted trie may have structural issue.");
        }
    } else {
        println!("Different insertion orders produce DIFFERENT root hashes!");
        println!("This indicates a bug in the trie algorithm (MPT should be order-independent).");
    }

    Ok(())
}
