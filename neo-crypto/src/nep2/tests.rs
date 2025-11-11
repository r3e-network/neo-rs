use super::{decrypt_nep2, encrypt_nep2, Nep2Error};
use crate::ecc256::PrivateKey;
use crate::scrypt::ScryptParams;
use neo_base::AddressVersion;

const TEST_PARAMS: ScryptParams = ScryptParams { n: 2, r: 1, p: 1 };
const TEST_VECTOR: &str = "6PYRzCDe46gkaR1E9AX3GyhLgQehypFvLG2KknbYjeNHQ3MZR2iqg8mcN3";

#[test]
fn nep2_encrypt_matches_vector() {
    let private = PrivateKey::new([1u8; 32]);
    let nep2 = encrypt_nep2(&private, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
    assert_eq!(nep2, TEST_VECTOR);
}

#[test]
fn nep2_decrypt_matches_vector() {
    let private =
        decrypt_nep2(TEST_VECTOR, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
    assert_eq!(private.as_be_bytes(), [1u8; 32]);
}

#[test]
fn nep2_roundtrip() {
    let private = PrivateKey::new([1u8; 32]);
    let nep2 = encrypt_nep2(&private, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
    let restored = decrypt_nep2(&nep2, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
    assert_eq!(restored.as_be_bytes(), private.as_be_bytes());
}

#[test]
fn nep2_invalid_password() {
    let result = decrypt_nep2(TEST_VECTOR, "wrong", AddressVersion::MAINNET, TEST_PARAMS);
    assert!(matches!(result, Err(Nep2Error::InvalidAddressHash)));
}
