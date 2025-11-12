use neo_base::Bytes;
use neo_store::{ColumnId, MemoryStore};

use crate::runtime::{
    context::tests::helpers::new_context,
    storage::{StorageFindItemKind, StorageFindOptions as Opt},
};

fn seed_store(store: &MemoryStore) {
    store.create_column(ColumnId::new("contract"));
    store
        .put(
            ColumnId::new("contract"),
            b"app:alpha".to_vec(),
            b"A".to_vec(),
        )
        .unwrap();
    store
        .put(
            ColumnId::new("contract"),
            b"app:beta".to_vec(),
            b"B".to_vec(),
        )
        .unwrap();
    store
        .put(
            ColumnId::new("contract"),
            b"other".to_vec(),
            b"ignored".to_vec(),
        )
        .unwrap();
}

#[test]
fn find_keys_only() {
    let mut store = MemoryStore::new();
    seed_store(&store);
    let ctx = new_context(&mut store);
    let items = ctx
        .find_storage_items(ColumnId::new("contract"), b"app:", Opt::KEYS_ONLY)
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].kind, StorageFindItemKind::Key(_)));
}

#[test]
fn find_remove_prefix_backwards() {
    let mut store = MemoryStore::new();
    seed_store(&store);
    let ctx = new_context(&mut store);
    let items = ctx
        .find_storage_items(
            ColumnId::new("contract"),
            b"app:",
            Opt::REMOVE_PREFIX | Opt::BACKWARDS,
        )
        .unwrap();
    match &items[0].kind {
        StorageFindItemKind::KeyValue { key, value } => {
            assert_eq!(key, &Bytes::from("beta"));
            assert_eq!(value, &Bytes::from("B"));
        }
        _ => panic!("expected key/value"),
    }
}
