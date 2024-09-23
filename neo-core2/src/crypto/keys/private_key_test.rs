extern crate hex;
extern crate num_bigint;
extern crate num_traits;
extern crate sha2;
extern crate secp256k1;
extern crate secp256r1;
extern crate test_case;
extern crate assert;

use hex::encode as hex_encode;
use num_bigint::BigInt;
use sha2::{Sha256, Digest};
use test_case::test_case;
use assert::{assert_eq, assert_ne, assert_err, assert_ok};

use crate::crypto::keys::{PrivateKey, PublicKey, NewPrivateKeyFromHex, NewPrivateKey, NewSecp256k1PrivateKey, NewPrivateKeyFromWIF};

#[test_case]
fn test_private_key() {
    for test_case in keytestcases::ARR.iter() {
        let priv_key_result = NewPrivateKeyFromHex(&test_case.private_key);
        if test_case.invalid {
            assert_err!(priv_key_result);
            continue;
        }

        let priv_key = priv_key_result.unwrap();
        assert_ok!(priv_key_result);
        let address = priv_key.address();
        assert_eq!(test_case.address, address);

        let wif = priv_key.wif();
        assert_eq!(test_case.wif, wif);
        let pub_key = priv_key.public_key();
        assert_eq!(hex_encode(pub_key.bytes()), test_case.public_key);
        let old_d = BigInt::from(priv_key.d.clone());
        priv_key.destroy();
        assert_ne!(old_d, priv_key.d);
    }
}

#[test_case]
fn test_new_private_key_on_curve() {
    let msg = vec![1, 2, 3];
    let mut hasher = Sha256::new();
    hasher.update(&msg);
    let h = hasher.finalize();

    #[test_case]
    fn secp256r1() {
        let p_result = NewPrivateKey();
        assert_ok!(p_result);
        let p = p_result.unwrap();
        p.public_key().verify(&p.sign(&msg), &h);
    }

    #[test_case]
    fn secp256k1() {
        let p_result = NewSecp256k1PrivateKey();
        assert_ok!(p_result);
        let p = p_result.unwrap();
        p.public_key().verify(&p.sign(&msg), &h);
    }
}

#[test_case]
fn test_private_key_from_wif() {
    for test_case in keytestcases::ARR.iter() {
        let key_result = NewPrivateKeyFromWIF(&test_case.wif);
        if test_case.invalid {
            assert_err!(key_result);
            continue;
        }

        let key = key_result.unwrap();
        assert_ok!(key_result);
        assert_eq!(test_case.private_key, key.to_string());
    }
}

#[test_case]
fn test_signing() {
    // These were taken from the rfcPage:https://tools.ietf.org/html/rfc6979#page-33
    //   public key: U = xG
    //Ux = 60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6
    //Uy = 7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299
    let private_key = NewPrivateKeyFromHex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721").unwrap();

    let data = private_key.sign(b"sample");

    let r = "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716";
    let s = "F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8";
    assert_eq!(r.to_lowercase() + s, hex_encode(data));
}
