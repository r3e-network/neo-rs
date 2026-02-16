use neo_core::smart_contract::stack_item_extract::{
    extract_bytes, extract_i32, extract_i64, extract_string, extract_u8, extract_u32,
};
use neo_vm::StackItem;

#[test]
fn extract_string_reads_utf8_bytes() {
    let item = StackItem::from_byte_string("neo".as_bytes());
    assert_eq!(extract_string(&item), Some("neo".to_string()));
}

#[test]
fn extract_string_rejects_invalid_utf8() {
    let item = StackItem::from_byte_string(vec![0xFF]);
    assert_eq!(extract_string(&item), None);
}

#[test]
fn extract_integers_apply_target_type_bounds() {
    let small = StackItem::from_int(7);
    let negative = StackItem::from_int(-1);
    let huge = StackItem::from_int(u64::MAX);

    assert_eq!(extract_u8(&small), Some(7));
    assert_eq!(extract_u32(&small), Some(7));
    assert_eq!(extract_i32(&small), Some(7));
    assert_eq!(extract_i64(&small), Some(7));

    assert_eq!(extract_u8(&negative), None);
    assert_eq!(extract_u32(&negative), None);
    assert_eq!(extract_i32(&huge), None);
}

#[test]
fn extract_bytes_reads_byte_string_values() {
    let bytes = vec![1_u8, 2, 3];
    let item = StackItem::from_byte_string(bytes.clone());
    assert_eq!(extract_bytes(&item), Some(bytes));
}
