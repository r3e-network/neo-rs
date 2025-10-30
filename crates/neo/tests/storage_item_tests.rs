use neo_core::neo_io::MemoryReader;
use neo_core::smart_contract::storage_item::StorageItem;

fn test_byte_array(length: usize, first_byte: u8) -> Vec<u8> {
    let mut data = vec![0x20; length];
    if let Some(first) = data.first_mut() {
        *first = first_byte;
    }
    data
}

#[test]
fn value_defaults_to_empty() {
    let uut = StorageItem::new();
    assert!(uut.get_value().is_empty());
}

#[test]
fn value_set_stores_bytes() {
    let mut uut = StorageItem::new();
    let value = vec![0x42, 0x32];
    uut.set_value(value.clone());

    let stored = uut.get_value();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0], value[0]);
    assert_eq!(stored[1], value[1]);
}

#[test]
fn size_uses_var_length_encoding() {
    let mut uut = StorageItem::new();
    uut.set_value(test_byte_array(10, 0x42));
    assert_eq!(uut.size(), 11);
}

#[test]
fn size_large_payload() {
    let mut uut = StorageItem::new();
    uut.set_value(test_byte_array(88, 0x42));
    assert_eq!(uut.size(), 89);
}

#[test]
fn clone_preserves_value() {
    let mut uut = StorageItem::new();
    uut.set_value(test_byte_array(10, 0x42));

    let cloned = uut.clone();
    let value = cloned.get_value();
    assert_eq!(value.len(), 10);
    assert_eq!(value[0], 0x42);
    for &byte in &value[1..] {
        assert_eq!(byte, 0x20);
    }
}

#[test]
fn deserialize_reads_all_bytes() {
    let data = vec![66, 32, 32, 32, 32, 32, 32, 32, 32, 32];
    let mut uut = StorageItem::new();
    let mut reader = MemoryReader::new(&data);
    uut.deserialize(&mut reader).expect("deserialize");

    let value = uut.get_value();
    assert_eq!(value, data);
}

#[test]
fn serialize_writes_current_value() {
    let bytes = test_byte_array(10, 0x42);
    let mut uut = StorageItem::new();
    uut.set_value(bytes.clone());

    let serialized = uut.serialize();
    assert_eq!(serialized, bytes);
}

#[test]
fn from_replica_copies_value() {
    let mut uut = StorageItem::new();
    uut.set_value(test_byte_array(10, 0x42));

    let mut dest = StorageItem::new();
    dest.from_replica(&uut);

    assert_eq!(uut.get_value(), dest.get_value());
}
