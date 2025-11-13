use neo_base::Bytes;
use neo_store::{ColumnId, MemoryStore};
use neo_vm::VmValue;

use crate::runtime::{context::tests::helpers::new_context, storage::StorageFindOptions as Opt};

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

fn serialized_pair(first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x40); // Array
    buf.push(0x02); // two elements
    buf.push(0x28); // ByteString
    buf.push(first.len() as u8);
    buf.extend_from_slice(first);
    buf.push(0x28); // ByteString
    buf.push(second.len() as u8);
    buf.extend_from_slice(second);
    buf
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
    assert!(matches!(items[0], VmValue::Bytes(_)));
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
    match &items[0] {
        VmValue::Array(values) => {
            assert_eq!(values.len(), 2);
            assert_eq!(values[0], VmValue::Bytes(Bytes::from("beta".as_bytes())));
            assert_eq!(values[1], VmValue::Bytes(Bytes::from("B".as_bytes())));
        }
        _ => panic!("expected key/value pair"),
    }
}

#[test]
fn find_deserialize_and_pick_field() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    store
        .put(
            ColumnId::new("contract"),
            b"app:item".to_vec(),
            serialized_pair(b"alpha", b"omega"),
        )
        .unwrap();
    let ctx = new_context(&mut store);
    let items = ctx
        .find_storage_items(
            ColumnId::new("contract"),
            b"app:",
            Opt::DESERIALIZE_VALUES | Opt::PICK_FIELD0 | Opt::VALUES_ONLY,
        )
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0], VmValue::Bytes(Bytes::from("alpha".as_bytes())));
}
