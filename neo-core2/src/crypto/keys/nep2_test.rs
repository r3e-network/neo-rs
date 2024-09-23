use std::str::FromStr;
use crate::crypto::keys::{NEP2Encrypt, NEP2Decrypt, validate_nep2_format, NEP2ScryptParams, PrivateKey};
use crate::keytestcases;
use assert_matches::assert_matches;

#[test]
fn test_nep2_encrypt() {
    for test_case in keytestcases::ARR.iter() {
        let priv_key = PrivateKey::from_str(test_case.private_key);
        if test_case.invalid {
            assert!(priv_key.is_err());
            continue;
        }

        assert!(priv_key.is_ok());
        let priv_key = priv_key.unwrap();

        let encrypted_wif = NEP2Encrypt(&priv_key, test_case.passphrase, &NEP2ScryptParams());
        assert!(encrypted_wif.is_ok());
        let encrypted_wif = encrypted_wif.unwrap();

        assert_eq!(test_case.encrypted_wif, encrypted_wif);
    }
}

#[test]
fn test_nep2_decrypt() {
    for test_case in keytestcases::ARR.iter() {
        let priv_key = NEP2Decrypt(test_case.encrypted_wif, test_case.passphrase, &NEP2ScryptParams());
        if test_case.invalid {
            assert!(priv_key.is_err());
            continue;
        }

        assert!(priv_key.is_ok());
        let priv_key = priv_key.unwrap();
        assert_eq!(test_case.private_key, priv_key.to_string());

        let wif = priv_key.to_wif();
        assert_eq!(test_case.wif, wif);

        let address = priv_key.to_address();
        assert_eq!(test_case.address, address);
    }
}

#[test]
fn test_nep2_decrypt_errors() {
    let p = "qwerty";

    // Not a base58-encoded value
    let s = "qazwsx";
    let result = NEP2Decrypt(s, p, &NEP2ScryptParams());
    assert!(result.is_err());

    // Valid base58, but not a NEP-2 format.
    let s = "KxhEDBQyyEFymvfJD96q8stMbJMbZUb6D1PmXqBWZDU2WvbvVs9o";
    let result = NEP2Decrypt(s, p, &NEP2ScryptParams());
    assert!(result.is_err());
}

#[test]
fn test_validate_nep2_format() {
    // Wrong length.
    let s = b"gobbledygook";
    assert!(validate_nep2_format(s).is_err());

    // Wrong header 1.
    let mut s = b"gobbledygookgobbledygookgobbledygookgob".to_vec();
    assert!(validate_nep2_format(&s).is_err());

    // Wrong header 2.
    s[0] = 0x01;
    assert!(validate_nep2_format(&s).is_err());

    // Wrong header 3.
    s[1] = 0x42;
    assert!(validate_nep2_format(&s).is_err());

    // OK
    s[2] = 0xe0;
    assert!(validate_nep2_format(&s).is_ok());
}
