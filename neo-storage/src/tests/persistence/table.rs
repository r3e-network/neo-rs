use crate::persistence::providers::MemoryStore;
use crate::persistence::read_only_store::RawReadOnlyStore;
use crate::persistence::{
    StorageItemCodec, StorageKeyCodec, Store, StoreMaintenanceBatch, Table, TableEncode,
    TableNamespace, TableProvider, U32BeCodec, U64BeCodec,
};
use crate::{StorageItem, StorageKey, StorageResult};

const COUNTER_PREFIX: &[u8] = b"test.counter.";

#[derive(Debug)]
struct CounterKeyCodec;

impl TableEncode<u32> for CounterKeyCodec {
    type Encoded<'a> = Vec<u8>;

    fn encode(value: &u32) -> StorageResult<Self::Encoded<'_>> {
        let mut encoded = Vec::with_capacity(COUNTER_PREFIX.len() + 4);
        encoded.extend_from_slice(COUNTER_PREFIX);
        encoded.extend_from_slice(&value.to_be_bytes());
        Ok(encoded)
    }
}

#[derive(Debug)]
struct CounterTable;

impl Table for CounterTable {
    type Key = u32;
    type Value = u64;
    type KeyCodec = CounterKeyCodec;
    type ValueCodec = U64BeCodec;

    const NAME: &'static str = "TestCounters";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

#[derive(Debug)]
struct ContractStorageTable;

impl Table for ContractStorageTable {
    type Key = StorageKey;
    type Value = StorageItem;
    type KeyCodec = StorageKeyCodec;
    type ValueCodec = StorageItemCodec;

    const NAME: &'static str = "NeoContractStorage";
    const NAMESPACE: TableNamespace = TableNamespace::Data;
}

#[test]
fn typed_maintenance_table_round_trips_without_changing_bytes() {
    let store = MemoryStore::new();
    let mut batch = StoreMaintenanceBatch::new();
    batch.put::<CounterTable>(&7, &42).expect("encode counter");
    assert!(
        store
            .try_commit_durable_maintenance(&batch)
            .expect("commit typed counter")
    );

    assert_eq!(store.table_get::<CounterTable>(&7).unwrap(), Some(42));
    assert!(store.table_contains::<CounterTable>(&7).unwrap());

    let mut raw_key = COUNTER_PREFIX.to_vec();
    raw_key.extend_from_slice(&7_u32.to_be_bytes());
    assert_eq!(
        store.maintenance_metadata(&raw_key).unwrap(),
        Some(42_u64.to_be_bytes().to_vec())
    );

    let mut delete = StoreMaintenanceBatch::new();
    delete.delete::<CounterTable>(&7).expect("encode deletion");
    assert!(
        store
            .try_commit_durable_maintenance(&delete)
            .expect("commit typed deletion")
    );
    assert_eq!(store.table_get::<CounterTable>(&7).unwrap(), None);
}

#[test]
fn typed_data_table_preserves_neo_storage_key_and_item_bytes() {
    let store = MemoryStore::new();
    let key = StorageKey::new(-4, vec![0x09, 0xAA, 0xBB]);
    let value = StorageItem::from_bytes(vec![0x11, 0x22, 0x33]);
    let mut batch = StoreMaintenanceBatch::new();
    batch
        .put::<ContractStorageTable>(&key, &value)
        .expect("encode Neo row");
    assert!(
        store
            .try_commit_durable_maintenance(&batch)
            .expect("commit Neo row")
    );

    assert_eq!(
        store
            .table_get::<ContractStorageTable>(&key)
            .unwrap()
            .map(|item| item.to_value()),
        Some(value.to_value())
    );
    assert_eq!(store.try_get_bytes(&key.to_array()), Some(value.to_value()));
}

#[test]
fn typed_table_reports_value_corruption_with_table_identity() {
    let store = MemoryStore::new();
    let mut raw_key = COUNTER_PREFIX.to_vec();
    raw_key.extend_from_slice(&9_u32.to_be_bytes());
    let mut corruption = StoreMaintenanceBatch::new();
    corruption.put_metadata(raw_key, vec![0x01, 0x02]);
    assert!(store.try_commit_durable_maintenance(&corruption).unwrap());

    let error = store
        .table_get::<CounterTable>(&9)
        .expect_err("short u64 must be rejected");
    assert!(error.to_string().contains("TestCounters"));
    assert!(error.to_string().contains("8-byte big-endian u64"));
}

#[test]
fn built_in_integer_codecs_are_order_preserving() {
    let lower = <U32BeCodec as TableEncode<u32>>::encode(&255).unwrap();
    let higher = <U32BeCodec as TableEncode<u32>>::encode(&256).unwrap();
    assert!(lower.as_slice() < higher.as_slice());
}
