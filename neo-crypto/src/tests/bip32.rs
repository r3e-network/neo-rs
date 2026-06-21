use super::Bip32Crypto;
use crate::{CryptoError, ECCurve};

#[test]
fn hmac_sha512_matches_bip32_master_seed_vector() {
    let seed = hex::decode("000102030405060708090a0b0c0d0e0f").expect("seed");
    let output = Bip32Crypto::hmac_sha512(b"Bitcoin seed", &seed).expect("hmac");

    assert_eq!(
        hex::encode(output),
        concat!(
            "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35",
            "873dff81c02f525623fd1fe5167eac3a55a049de3d314bb42ee227ffed37d508"
        )
    );
}

#[test]
fn add_private_keys_mod_order_adds_small_scalars() {
    let mut left_factor = [0u8; 32];
    left_factor[31] = 1;
    let mut parent = [0u8; 32];
    parent[31] = 2;

    let child =
        Bip32Crypto::add_private_keys_mod_order(&left_factor, &parent, ECCurve::Secp256r1)
            .expect("secp256r1 child");
    assert_eq!(child[31], 3);

    let child =
        Bip32Crypto::add_private_keys_mod_order(&left_factor, &parent, ECCurve::Secp256k1)
            .expect("secp256k1 child");
    assert_eq!(child[31], 3);
}

#[test]
fn add_private_keys_mod_order_rejects_invalid_inputs() {
    let left_factor = [0xffu8; 32];
    let parent = [1u8; 32];

    assert!(matches!(
        Bip32Crypto::add_private_keys_mod_order(&left_factor, &parent, ECCurve::Secp256r1),
        Err(CryptoError::InvalidArgument { .. })
    ));

    assert!(matches!(
        Bip32Crypto::add_private_keys_mod_order(&[1u8; 32], &parent, ECCurve::Ed25519),
        Err(CryptoError::InvalidArgument { .. })
    ));
}
