use neo_core::state_service::state_store::{MemoryStateStoreBackend, StateStoreSnapshot};
use neo_crypto::mpt_trie::Trie;
use neo_primitives::UInt256;
use std::io::{self, BufRead};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    for line in io::stdin().lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let key_hex = parts.next().ok_or("missing key hex")?;
        let value_hex = parts.next().unwrap_or("").trim();
        if key_hex.is_empty() {
            return Err(format!("missing key hex in line: {trimmed}").into());
        }
        if value_hex.contains(char::is_whitespace) {
            return Err(format!("unexpected extra fields in line: {trimmed}").into());
        }

        let key = hex::decode(key_hex)?;
        let value = hex::decode(value_hex)?;
        entries.push((key, value));
    }

    let store = Arc::new(MemoryStateStoreBackend::new());
    let snapshot = Arc::new(StateStoreSnapshot::new(store));
    let mut trie = Trie::new(snapshot, None, true);

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in &entries {
        trie.put(key, value)?;
    }

    let root = trie.root_hash().unwrap_or_else(UInt256::zero);
    println!("{}", root);
    Ok(())
}
