use super::*;

#[test]
fn test_constant_time_hash_eq() {
    let hash1 = Crypto::sha256(b"test message");
    let hash2 = Crypto::sha256(b"test message");
    let hash3 = Crypto::sha256(b"different message");

    assert!(CtCompare::ct_hash_eq(&hash1, &hash2));
    assert!(CtCompare::ct_hash_slice_eq(&hash1, &hash2));

    assert!(!CtCompare::ct_hash_eq(&hash1, &hash3));
    assert!(!CtCompare::ct_hash_slice_eq(&hash1, &hash3));

    assert!(CtCompare::ct_hash_eq(&hash1, &hash1));
    assert!(CtCompare::ct_hash_slice_eq(&hash1, &hash1));
}

#[test]
fn test_constant_time_slice_eq_different_lengths() {
    let a = [0u8; 32];
    let b = [0u8; 64];
    assert!(!CtCompare::ct_hash_slice_eq(&a, &b));
}

#[test]
fn test_constant_time_single_byte_diff() {
    let a = [0u8; 32];
    let mut b = [0u8; 32];

    assert!(CtCompare::ct_hash_eq(&a, &b));

    b[0] = 1;
    assert!(!CtCompare::ct_hash_eq(&a, &b));

    b[0] = 0;
    b[31] = 1;
    assert!(!CtCompare::ct_hash_eq(&a, &b));

    b[31] = 0;
    b[15] = 1;
    assert!(!CtCompare::ct_hash_eq(&a, &b));
}

#[test]
fn test_sha256() {
    let hash = Crypto::sha256(b"hello");
    assert_eq!(hash.len(), 32);
    let expected =
        hex::decode("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824").unwrap();
    assert_eq!(hash.to_vec(), expected);
}

#[test]
fn test_sha256_hasher_matches_one_shot_hash() {
    let mut hasher = Sha256Hasher::new();
    hasher.update(b"he");
    hasher.update(b"llo");

    assert_eq!(hasher.finalize(), Crypto::sha256(b"hello"));
}

#[test]
fn test_sha512() {
    let hash = Crypto::sha512(b"hello");
    assert_eq!(hash.len(), 64);
}

#[test]
fn test_ripemd160() {
    let hash = Crypto::ripemd160(b"hello");
    assert_eq!(hash.len(), 20);
}

#[test]
fn test_hash160() {
    let hash = Crypto::hash160(b"hello");
    assert_eq!(hash.len(), 20);
    let sha256 = Crypto::sha256(b"hello");
    let expected = Crypto::ripemd160(&sha256);
    assert_eq!(hash, expected);
}

#[test]
fn test_hash256() {
    let hash = Crypto::hash256(b"hello");
    assert_eq!(hash.len(), 32);
    let first = Crypto::sha256(b"hello");
    let expected = Crypto::sha256(&first);
    assert_eq!(hash, expected);
}

#[test]
fn test_keccak256() {
    let hash = Crypto::keccak256(b"hello");
    assert_eq!(hash.len(), 32);
}

#[test]
fn test_sha3_256() {
    let hash = Crypto::sha3_256(b"hello world");
    let expected =
        hex::decode("644bcc7e564373040999aac89e7622f3ca71fba1d972fd94a31c3bfbf24e3938").unwrap();
    assert_eq!(hash.to_vec(), expected);
}

#[test]
fn test_sha3_512() {
    let hash = Crypto::sha3_512(b"hello world");
    let expected = hex::decode("840006653e9ac9e95117a15c915caab81662918e925de9e004f774ff82d7079a40d4d27b1b372657c61d46d470304c88c788b3a4527ad074d1dccbee5dbaa99a")
        .unwrap();
    assert_eq!(hash.to_vec(), expected);
}

#[test]
fn test_blake2b() {
    let hash = Crypto::blake2b(b"hello");
    assert_eq!(hash.len(), 64);
}

#[test]
fn test_blake2s() {
    let hash = Crypto::blake2s(b"hello");
    assert_eq!(hash.len(), 32);
}

#[test]
fn test_blake2b_512() {
    let hash = Crypto::blake2b_512(b"hello world", None).unwrap();
    let expected = hex::decode("021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0")
        .unwrap();
    assert_eq!(hash.to_vec(), expected);

    let salt = b"0123456789abcdef";
    let hash = Crypto::blake2b_512(b"hello world", Some(salt)).unwrap();
    let expected = hex::decode("d986f099932b14a65ebc5a6fb1b8bff8d05b6924a4ff74d4972949b880c1f74b5ab263357f332726d98fac3cabeacf415099f1a2a9b97b66cd989ca865539640")
        .unwrap();
    assert_eq!(hash.to_vec(), expected);

    assert!(Crypto::blake2b_512(b"abc", Some(&[0u8; 15])).is_err());
    assert!(Crypto::blake2b_512(b"abc", Some(&[0u8; 17])).is_err());
}

#[test]
fn test_blake2b_256() {
    let hash = Crypto::blake2b_256(b"hello world", None).unwrap();
    let expected =
        hex::decode("256c83b297114d201b30179f3f0ef0cace9783622da5974326b436178aeef610").unwrap();
    assert_eq!(hash.to_vec(), expected);

    let salt = b"0123456789abcdef";
    let hash = Crypto::blake2b_256(b"hello world", Some(salt)).unwrap();
    let expected =
        hex::decode("779c5f2194a9c2c03e73e3ffcf3e1508dd83cb85cd861029415ab961a755cc4e").unwrap();
    assert_eq!(hash.to_vec(), expected);

    assert!(Crypto::blake2b_256(b"abc", Some(&[0u8; 15])).is_err());
    assert!(Crypto::blake2b_256(b"abc", Some(&[0u8; 17])).is_err());
}

#[test]
fn hash_algorithm_neo_protocol_bytes_match_csharp_v3_10() {
    assert_eq!(HashAlgorithm::Sha256.to_neo_byte(), Some(0x00));
    assert_eq!(HashAlgorithm::Keccak256.to_neo_byte(), Some(0x01));
    assert_eq!(HashAlgorithm::Sha512.to_neo_byte(), Some(0x02));

    assert_eq!(
        HashAlgorithm::from_neo_byte(0x00),
        Some(HashAlgorithm::Sha256)
    );
    assert_eq!(
        HashAlgorithm::from_neo_byte(0x01),
        Some(HashAlgorithm::Keccak256)
    );
    assert_eq!(
        HashAlgorithm::from_neo_byte(0x02),
        Some(HashAlgorithm::Sha512)
    );

    assert_eq!(HashAlgorithm::Ripemd160.to_neo_byte(), None);
    assert_eq!(HashAlgorithm::Blake2b.to_neo_byte(), None);
    assert_eq!(HashAlgorithm::Blake2s.to_neo_byte(), None);
    assert_eq!(HashAlgorithm::from_neo_byte(0x03), None);
}

#[test]
fn test_hash_algorithm() {
    let data = b"test data";

    assert_eq!(Crypto::hash(HashAlgorithm::Sha256, data).len(), 32);
    assert_eq!(Crypto::hash(HashAlgorithm::Sha512, data).len(), 64);
    assert_eq!(Crypto::hash(HashAlgorithm::Ripemd160, data).len(), 20);
    assert_eq!(Crypto::hash(HashAlgorithm::Keccak256, data).len(), 32);
    assert_eq!(Crypto::hash(HashAlgorithm::Blake2b, data).len(), 64);
    assert_eq!(Crypto::hash(HashAlgorithm::Blake2s, data).len(), 32);
}
