use super::*;

use crate::{MptChange, MptStore};

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

#[test]
fn mpt_state_provider_factory_opens_state_view_at_height() {
    let store = MptStore::new(true);
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA], b"v1")])
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA], b"v2")])
        .expect("block 2 applies");
    let provider = MptStateProviderFactory::new(&store);

    let view1 = StateProviderFactory::state_view_at_height(&provider, 1)
        .expect("state view")
        .expect("height 1 root");
    let view2 = StateProviderFactory::state_view_at_height(&provider, 2)
        .expect("state view")
        .expect("height 2 root");
    let current = StateProviderFactory::current_state_view(&provider)
        .expect("state view")
        .expect("current root");

    assert_eq!(StateView::height(&view1), Some(1));
    assert_eq!(StateView::root_hash(&view1), root1);
    assert_eq!(
        StateView::storage_value(&view1, &storage_key(5, &[0xAA])).unwrap(),
        Some(b"v1".to_vec())
    );
    assert_eq!(StateView::height(&view2), Some(2));
    assert_eq!(StateView::root_hash(&view2), root2);
    assert_eq!(
        StateView::storage_value(&view2, &storage_key(5, &[0xAA])).unwrap(),
        Some(b"v2".to_vec())
    );
    assert_eq!(StateView::height(&current), Some(2));
    assert_eq!(StateView::root_hash(&current), root2);
}

#[test]
fn mpt_state_view_keeps_one_snapshot_for_root_gate_and_trie_reads() {
    let store = MptStore::new(true);
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA], b"v1")])
        .expect("block 1 applies");
    let provider = MptStateProviderFactory::new(&store);
    let view = StateProviderFactory::state_view_at_root(&provider, root1).expect("state view");

    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA], b"v2")])
        .expect("block 2 applies");

    assert_eq!(StateView::current_local_root_hash(&view), Some(root1));
    assert_eq!(StateView::root_hash(&view), root1);
    assert_ne!(StateView::root_hash(&view), root2);
    assert_eq!(
        StateView::storage_value(&view, &storage_key(5, &[0xAA])).unwrap(),
        Some(b"v1".to_vec())
    );
}
