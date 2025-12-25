use neo_core::smart_contract::find_options::FindOptions;
use neo_core::smart_contract::iterators::i_iterator::IIterator;
use neo_core::smart_contract::iterators::storage_iterator::StorageIterator;
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_vm::StackItem;

#[test]
fn storage_iterator_dispose_is_safe() {
    let mut iterator = StorageIterator::new(Vec::new(), 0, FindOptions::None);
    iterator.dispose();
    assert!(!iterator.next());
}

#[test]
fn storage_iterator_value_values_only() {
    let key = StorageKey::new(0, vec![0u8]);
    let value = StorageItem::from_bytes(vec![0u8]);
    let mut iterator = StorageIterator::new(vec![(key, value)], 0, FindOptions::ValuesOnly);

    assert!(iterator.next());
    assert_eq!(iterator.value(), StackItem::from_byte_string(vec![0u8]));
}
