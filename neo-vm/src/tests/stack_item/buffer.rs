use super::*;

#[test]
fn to_integer_follows_vm_integer_size_and_signed_bytes() {
    let negative = Buffer::new(vec![0x00, 0x80]);
    assert_eq!(negative.to_integer().unwrap(), BigInt::from(-32768));

    let too_large = Buffer::new(vec![0; 33]);
    assert!(too_large.to_integer().is_err());
}
