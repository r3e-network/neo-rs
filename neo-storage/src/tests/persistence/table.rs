use super::*;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::persistence::{StoreTableRead, Table, TableCodec, TableReader};
use crate::{StorageItem, StorageKey};

struct ContractStorageTable;

impl Table for ContractStorageTable {
    type Key = StorageKey;
    type Value = StorageItem;

    const NAME: &'static str = "contract_storage";
}

#[test]
fn table_reader_preserves_existing_storage_key_and_item_bytes() {
    let mut store = MemoryStore::new();
    let key = StorageKey::new(42, vec![0xAA, 0xBB]);
    let value = StorageItem::from_bytes(vec![0xCA, 0xFE]);

    store
        .put(TableCodec::encode(&key), TableCodec::encode(&value))
        .expect("put raw storage row");

    let reader = TableReader::<ContractStorageTable>::new(&store);
    let decoded = reader
        .get(&key)
        .expect("typed table read")
        .expect("row present");

    assert_eq!(decoded, value);
    assert_eq!(TableCodec::encode(&key), key.to_array());
    assert_eq!(TableCodec::encode(&decoded), value.to_value());
}

#[test]
fn table_reader_reports_decode_errors_with_table_name() {
    #[derive(Debug)]
    struct FailingValue;
    struct BrokenTable;

    impl TableCodec for FailingValue {
        fn encode(&self) -> Vec<u8> {
            Vec::new()
        }

        fn decode(_bytes: &[u8]) -> crate::StorageResult<Self> {
            Err(crate::StorageError::invalid_data("bad value"))
        }
    }

    impl Table for BrokenTable {
        type Key = Vec<u8>;
        type Value = FailingValue;

        const NAME: &'static str = "broken_table";
    }

    let mut store = MemoryStore::new();
    store
        .put(vec![0x01], vec![0xFF])
        .expect("put raw storage row");

    let err = TableReader::<BrokenTable>::new(&store)
        .get(&vec![0x01])
        .expect_err("decode should fail");

    assert!(
        err.to_string().contains("broken_table"),
        "table decode error should include table name: {err}"
    );
}
