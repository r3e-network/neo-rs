use super::*;

#[test]
fn mainnet_height_5549_visible_storage_set_matches_reference_root() {
    let fixture = include_str!("../../../../fixtures/mpt/mainnet-height-5549-storage.json");
    let entries: Vec<MainnetStorageEntry> = serde_json::from_str(fixture).expect("fixture parses");
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);

    for entry in &entries {
        let key = hex::decode(&entry.storage_key_hex).expect("storage key hex");
        let value = hex::decode(&entry.value_hex).expect("storage value hex");
        trie.put(&key, &value).expect("fixture entry inserts");
    }

    assert_eq!(
        entries.len(),
        132,
        "fixture should cover every visible leaf"
    );
    assert_eq!(
        trie.root_hash().expect("fixture root"),
        UInt256::parse("0x559cf882e478a11af654ba826f85302e81a4e428466d1973d49b6ae0cf8919d7")
            .expect("reference state root parses"),
        "rebuilding the verified height-5549 visible storage set must match the Neo reference root"
    );
}

#[test]
fn mainnet_height_5549_missing_next_candidate_entry_reproduces_local_wrong_root() {
    let fixture = include_str!("../../../../fixtures/mpt/mainnet-height-5549-storage.json");
    let entries: Vec<MainnetStorageEntry> = serde_json::from_str(fixture).expect("fixture parses");
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);
    let missing_key = "0100000077ae4d8fb978106d6872f38de96a73f50c53e95d32";

    for entry in entries
        .iter()
        .filter(|entry| entry.storage_key_hex != missing_key)
    {
        let key = hex::decode(&entry.storage_key_hex).expect("storage key hex");
        let value = hex::decode(&entry.value_hex).expect("storage value hex");
        trie.put(&key, &value).expect("fixture entry inserts");
    }

    assert_eq!(
        trie.root_hash().expect("fixture root"),
        UInt256::parse("0x9b55315ab4c734f33bb24a167903b4d6eacd6d28b6237fb2cb6e35e6b1a3a372")
            .expect("local wrong root parses"),
        "omitting the NEXT/NeoLine candidate storage entry reproduces the known local height-5549 root"
    );
}

#[derive(serde::Deserialize)]
struct MainnetStorageEntry {
    storage_key_hex: String,
    value_hex: String,
}
