use super::*;

/// `0xf0`, the MPT-node key prefix used by `neo_crypto::mpt_trie`
/// and the C# `Cache`.
const NODE_PREFIX: u8 = 0xf0;

fn storage_key(id: i32, suffix: &[u8]) -> Vec<u8> {
    let mut key = id.to_le_bytes().to_vec();
    key.extend_from_slice(suffix);
    key
}

fn put(id: i32, suffix: &[u8], value: &[u8]) -> MptChange {
    MptChange::Put {
        key: storage_key(id, suffix),
        value: value.to_vec(),
    }
}

fn delete(id: i32, suffix: &[u8]) -> MptChange {
    MptChange::Delete {
        key: storage_key(id, suffix),
    }
}

fn two_block_store(full_state: bool) -> (Arc<MptStore>, UInt256, UInt256) {
    let store = Arc::new(MptStore::new(full_state));
    let root1 = store
        .apply_block_changes(
            1,
            None,
            &[
                put(5, &[0xAA, 0x01], b"v1"),
                put(5, &[0xAA, 0x02], b"v2"),
                put(5, &[0xBB, 0x01], b"other"),
            ],
        )
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(
            2,
            Some(root1),
            &[
                put(5, &[0xAA, 0x01], b"v1-updated"),
                put(5, &[0xAA, 0x03], b"v3"),
                delete(5, &[0xAA, 0x02]),
            ],
        )
        .expect("block 2 applies");
    (store, root1, root2)
}

#[test]
fn apply_block_changes_advances_root_and_records() {
    let (store, root1, root2) = two_block_store(true);
    assert_ne!(root1, root2);

    // Per-block records under the C# key scheme.
    let record1 = store.get_state_root(1).expect("block 1 record");
    assert_eq!(*record1.root_hash(), root1);
    assert_eq!(record1.index(), 1);
    let record2 = store.get_state_root(2).expect("block 2 record");
    assert_eq!(*record2.root_hash(), root2);

    assert_eq!(store.current_local_root_index(), Some(2));
    assert_eq!(store.current_local_root_hash(), Some(root2));
    assert!(store.get_state_root(3).is_none());
}

#[test]
fn kv_layout_matches_csharp_key_scheme() {
    let (store, _root1, root2) = two_block_store(true);
    let kv = store.kv.read();

    // 0x01 || u32 BE -> state-root record (C# Keys.StateRoot).
    let record = kv
        .get(&[0x01, 0, 0, 0, 2][..])
        .expect("state-root record for block 2");
    assert_eq!(record[0], crate::state_root::CURRENT_VERSION);
    assert_eq!(&record[1..5], &2u32.to_le_bytes());
    assert_eq!(&record[5..37], &root2.to_bytes());
    // Unwitnessed local root: a single var-int 0 witness count.
    assert_eq!(&record[37..], &[0x00]);

    // 0x02 -> current local root index, little-endian u32.
    assert_eq!(
        kv.get(&[0x02][..]).map(Vec::as_slice),
        Some(&2u32.to_le_bytes()[..])
    );

    // MPT nodes live under 0xf0 || node hash.
    assert!(
        kv.keys()
            .any(|key| key.len() == 33 && key[0] == NODE_PREFIX),
        "trie nodes must be persisted under the 0xf0 prefix"
    );
}

#[test]
fn full_state_serves_both_historical_roots() {
    let (store, root1, root2) = two_block_store(true);

    let mut trie1 = store.open_trie(Some(root1));
    assert_eq!(
        trie1.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1".to_vec())
    );
    assert_eq!(
        trie1.get(&storage_key(5, &[0xAA, 0x02])).expect("get"),
        Some(b"v2".to_vec())
    );
    assert_eq!(
        trie1.get(&storage_key(5, &[0xAA, 0x03])).expect("get"),
        None
    );

    let mut trie2 = store.open_trie(Some(root2));
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1-updated".to_vec())
    );
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x02])).expect("get"),
        None
    );
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x03])).expect("get"),
        Some(b"v3".to_vec())
    );
}

#[test]
fn pruning_mode_keeps_only_current_root() {
    let (store, root1, root2) = two_block_store(false);

    // The current root stays fully resolvable.
    let mut trie2 = store.open_trie(Some(root2));
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1-updated".to_vec())
    );

    // Nodes superseded by block 2 were pruned, so the old root can
    // no longer resolve the rewritten path.
    let mut trie1 = store.open_trie(Some(root1));
    assert!(
        trie1.get(&storage_key(5, &[0xAA, 0x01])).is_err(),
        "block-1 path through pruned nodes must fail to resolve"
    );
}

#[test]
fn proof_round_trips_through_verify() {
    let (store, _root1, root2) = two_block_store(true);
    let key = storage_key(5, &[0xAA, 0x03]);

    let mut trie = store.open_trie(Some(root2));
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof exists");
    let value = Trie::<MptReadSnapshot>::verify_proof(root2, &key, &proof).expect("proof verifies");
    assert_eq!(value, b"v3".to_vec());
}

#[test]
fn deterministic_roots_across_stores() {
    let (_, root1_a, root2_a) = two_block_store(true);
    let (_, root1_b, root2_b) = two_block_store(true);
    assert_eq!(root1_a, root1_b);
    assert_eq!(root2_a, root2_b);
    // Insertion into the trie is order-independent for a set of
    // distinct keys, and pruning mode must not change the root.
    let (_, root1_c, root2_c) = two_block_store(false);
    assert_eq!(root1_a, root1_c);
    assert_eq!(root2_a, root2_c);
}

#[test]
fn backed_store_reopens_local_state_root_records() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;

    let backing: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store = Arc::new(MptStore::from_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    let reopened = MptStore::from_store(Arc::clone(&backing), true).expect("reopen store");
    assert_eq!(reopened.current_local_root_index(), Some(1));
    assert_eq!(reopened.current_local_root_hash(), Some(root1));
    assert_eq!(
        reopened
            .get_state_root(1)
            .expect("reopened state root")
            .root_hash(),
        &root1
    );
}

#[test]
fn empty_change_set_preserves_root() {
    let (store, _root1, root2) = two_block_store(true);
    let root3 = store
        .apply_block_changes(3, Some(root2), &[])
        .expect("empty block applies");
    assert_eq!(root3, root2);
    assert_eq!(store.current_local_root_index(), Some(3));
    assert_eq!(store.current_local_root_hash(), Some(root2));
}

/// Cross-pinned against the official C# implementation: the vector
/// below was produced by the published `Neo.Cryptography.MPT` 3.9.2
/// package (the NuGet build of `Neo.Cryptography.MPTTrie`, compiled
/// against `Neo` 3.9.1 — the reference version vendored under
/// `neo_csharp/`; the MPTTrie project itself is not vendored).
/// A `MemoryStore`-backed `Trie` applied exactly the
/// [`two_block_store`] change sets and dumped `Trie.Root.Hash`
/// after each block plus `Trie.TryGetProof` for `(5, 0xAA03)`
/// under the block-2 root.
#[test]
fn roots_and_proof_match_csharp_reference_vector() {
    const CSHARP_ROOT1: &str = "0xe70c4472181cd5be21ca64895f59c36556d6c4da225e261fb0c9cba9dd23b13a";
    const CSHARP_ROOT2: &str = "0x0d62b916694bab0b56983f65ef2cf175851029905698db802de1727861f7b338";
    const CSHARP_PROOF_NODES: [&str; 5] = [
        "0004033e0c12347ed09c36b37110a651a9711a8896b5fc318f2205b15d21d7948cd3e404034f77a7c5f2a8bb02df3a9e0ca5357e2748ba585e9f5df589e89d15c9afadebe504040404040404040404040404",
        "0004040404040404040404036c7955a84481137bb548b266e4b840675691dda220ebcc44625b491516f32639039b7f19f121765d6b6e80128cb6273503af1042384b119b89e85d7628924df29e0404040404",
        "01020a0003eb4cad27f7ba97ade5e2119a0464e4047823e85caae54fb4fff3e1addc07751d",
        "0108000500000000000003f5d20f5a796a7c89ee22dadf440cc1180b853cc167576636e096af9649bc629e",
        "02027633",
    ];

    let (store, root1, root2) = two_block_store(true);
    assert_eq!(
        root1,
        UInt256::parse(CSHARP_ROOT1).expect("pinned root1 parses"),
        "block-1 root must match the C# reference"
    );
    assert_eq!(
        root2,
        UInt256::parse(CSHARP_ROOT2).expect("pinned root2 parses"),
        "block-2 root must match the C# reference"
    );

    // The C#-emitted proof verifies through the Rust verifier and
    // yields the C#-verified value.
    let key = storage_key(5, &[0xAA, 0x03]);
    let csharp_proof: std::collections::HashSet<Vec<u8>> = CSHARP_PROOF_NODES
        .iter()
        .map(|node| hex::decode(node).expect("pinned node hex"))
        .collect();
    let value = Trie::<MptReadSnapshot>::verify_proof(root2, &key, &csharp_proof)
        .expect("C# proof verifies in Rust");
    assert_eq!(value, b"v3".to_vec());

    // And the Rust prover emits the identical node set.
    let mut trie = store.open_trie(Some(root2));
    let rust_proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof exists");
    let mut rust_nodes: Vec<String> = rust_proof.iter().map(hex::encode).collect();
    rust_nodes.sort_unstable();
    assert_eq!(rust_nodes, CSHARP_PROOF_NODES, "proof node set must match");
}

#[test]
fn snapshot_preserves_pruned_generation_for_in_flight_readers() {
    // Pruning mode: applying a block deletes the nodes the change
    // set superseded. A snapshot taken before the apply must keep
    // resolving the old root (the C# immutable store snapshot).
    let store = Arc::new(MptStore::new(false));
    let key = storage_key(5, &[0xAA, 0x01]);
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    let snapshot = store.snapshot();
    assert_eq!(snapshot.current_local_root_index(), Some(1));
    assert_eq!(snapshot.current_local_root_hash(), Some(root1));

    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x01], b"v2")])
        .expect("block 2 applies");
    assert_ne!(root1, root2);

    // The frozen view still serves the pre-apply generation...
    let mut snapshot_trie = snapshot.open_trie(Some(root1));
    assert_eq!(
        snapshot_trie.get(&key).expect("snapshot read"),
        Some(b"v1".to_vec()),
        "snapshot must keep resolving the generation it captured"
    );
    // ...and still reports the index/hash it captured.
    assert_eq!(snapshot.current_local_root_index(), Some(1));

    // While the live store has pruned the superseded nodes.
    let mut live_trie = store.open_trie(Some(root1));
    assert!(
        live_trie.get(&key).is_err(),
        "live store must have pruned the block-1 path"
    );
    let mut current_trie = store.open_trie(Some(root2));
    assert_eq!(
        current_trie.get(&key).expect("current read"),
        Some(b"v2".to_vec())
    );
}

#[test]
fn read_snapshot_rejects_writes() {
    let (store, _root1, root2) = two_block_store(true);
    let snapshot = store.snapshot();

    // Direct store-surface writes are refused...
    assert!(MptStoreSnapshot::put(&*snapshot, vec![0x01], vec![0x02]).is_err());
    assert!(MptStoreSnapshot::delete(&*snapshot, vec![0x01]).is_err());

    // ...and a trie commit over the snapshot fails instead of
    // silently mutating a frozen view.
    let mut trie = snapshot.open_trie(Some(root2));
    trie.put(&storage_key(5, &[0xEE]), b"nope")
        .expect("puts stage in the trie cache");
    assert!(trie.commit().is_err(), "snapshot commit must be rejected");
}

#[test]
fn concurrent_apply_and_snapshot_reads_stay_consistent() {
    // Writer applies pruning-mode blocks that rewrite the same key
    // set each time; readers snapshot, then verify every key under
    // the snapshot's own current root carries that block's value.
    // Without snapshot isolation the pruning writer deletes nodes
    // out from under the readers' walks.
    const BLOCKS: u32 = 50;
    const KEYS: u8 = 16;

    let store = Arc::new(MptStore::new(false));
    let value_for = |block: u32| block.to_le_bytes().to_vec();

    let writer = {
        let store = Arc::clone(&store);
        std::thread::spawn(move || {
            let mut root = None;
            for block in 1..=BLOCKS {
                let changes: Vec<MptChange> = (0..KEYS)
                    .map(|i| put(5, &[0xAA, i], &value_for(block)))
                    .collect();
                let new_root = store
                    .apply_block_changes(block, root, &changes)
                    .expect("block applies");
                root = Some(new_root);
            }
        })
    };

    let readers: Vec<_> = (0..4)
        .map(|_| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                let mut observed = 0u32;
                while observed < BLOCKS {
                    let snapshot = store.snapshot();
                    let Some(index) = snapshot.current_local_root_index() else {
                        std::thread::yield_now();
                        continue;
                    };
                    let root = *snapshot
                        .get_state_root(index)
                        .expect("snapshot has its own root record")
                        .root_hash();
                    let mut trie = snapshot.open_trie(Some(root));
                    for i in 0..KEYS {
                        let value = trie
                            .get(&storage_key(5, &[0xAA, i]))
                            .expect("snapshot walk must never lose nodes")
                            .expect("key present in every block");
                        assert_eq!(
                            value,
                            value_for(index),
                            "all keys in one snapshot must carry the same block's value"
                        );
                    }
                    observed = observed.max(index);
                }
            })
        })
        .collect();

    writer.join().expect("writer thread");
    for reader in readers {
        reader.join().expect("reader thread");
    }

    assert_eq!(store.current_local_root_index(), Some(BLOCKS));
}

#[test]
fn find_enumerates_prefix_in_order() {
    let (store, _root1, root2) = two_block_store(true);
    let mut trie = store.open_trie(Some(root2));
    let prefix = storage_key(5, &[0xAA]);
    let entries = trie.find(&prefix, None).expect("find");
    let keys: Vec<Vec<u8>> = entries.iter().map(|e| e.key.clone()).collect();
    assert_eq!(
        keys,
        vec![storage_key(5, &[0xAA, 0x01]), storage_key(5, &[0xAA, 0x03]),]
    );

    // Resume strictly after the first key.
    let entries = trie
        .find(&prefix, Some(&storage_key(5, &[0xAA, 0x01])))
        .expect("find with from");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, storage_key(5, &[0xAA, 0x03]));
    assert_eq!(entries[0].value, b"v3".to_vec());
}
