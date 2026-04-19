use neo_vm::stack_item::byte_string::ByteString;

#[test]
fn user_hash_274157_byte8_roundtrip() {
    // The exact user hash from block 274,157 (LE)
    let original = vec![
        0x20, 0xaa, 0x6b, 0x22, 0x8d, 0xe8, 0xbd, 0xe3,
        0xc9, 0x18, 0x89, 0x7b, 0x48, 0x54, 0xcb, 0xf9,
        0xf7, 0xdd, 0xba, 0x3e,
    ];
    let bs = ByteString::new(original.clone());
    let int = bs.to_integer().unwrap();
    eprintln!("int = {}", int);
    let back = int.to_signed_bytes_le();
    eprintln!("orig: {:02x?}", original);
    eprintln!("back: {:02x?}", back);
    assert_eq!(back, original, "ByteString→Integer→bytes should roundtrip");
}

#[test]
fn single_byte_0xc9_roundtrip() {
    // Single byte 0xc9 — high bit set
    let bs = ByteString::new(vec![0xc9]);
    let int = bs.to_integer().unwrap();
    eprintln!("0xc9 as int = {}", int);
    let back = int.to_signed_bytes_le();
    eprintln!("back: {:02x?}", back);
    assert_eq!(back, vec![0xc9]);
}

#[test]
fn high_bit_set_slice_roundtrip() {
    // First 9 bytes: [..., 0xc9] — last byte has bit 7 set
    let slice = vec![0x20, 0xaa, 0x6b, 0x22, 0x8d, 0xe8, 0xbd, 0xe3, 0xc9];
    let bs = ByteString::new(slice.clone());
    let int = bs.to_integer().unwrap();
    eprintln!("slice int = {}", int);
    let back = int.to_signed_bytes_le();
    eprintln!("back: {:02x?}", back);
    assert_eq!(back, slice);
}
