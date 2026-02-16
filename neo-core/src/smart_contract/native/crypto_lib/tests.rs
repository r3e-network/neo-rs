use super::*;

#[test]
fn test_sha256() {
    let lib = CryptoLib::new();
    let data = b"hello world".to_vec();
    let result = lib.sha256(&[data]).unwrap();
    assert_eq!(result.len(), 32);
}

#[test]
fn test_ripemd160() {
    let lib = CryptoLib::new();
    let data = b"hello world".to_vec();
    let result = lib.ripemd160(&[data]).unwrap();
    assert_eq!(result.len(), 20);
}
