use super::Bls12381Crypto;

const PRIVATE_KEY: [u8; 32] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32,
];
const SECOND_PRIVATE_KEY: [u8; 32] = [
    33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
    57, 58, 59, 60, 61, 62, 63, 64,
];
const MESSAGE: &[u8] = b"neo-rs bls compatibility";

fn decode_array<const N: usize>(hex: &str) -> [u8; N] {
    hex::decode(hex).unwrap().try_into().unwrap()
}

#[test]
fn bls12381_compatibility_vector() {
    let public_key = Bls12381Crypto::derive_public_key(&PRIVATE_KEY).unwrap();
    let signature = Bls12381Crypto::sign(MESSAGE, &PRIVATE_KEY).unwrap();
    let second_public_key = Bls12381Crypto::derive_public_key(&SECOND_PRIVATE_KEY).unwrap();
    let second_signature = Bls12381Crypto::sign(MESSAGE, &SECOND_PRIVATE_KEY).unwrap();
    let aggregated = Bls12381Crypto::aggregate_signatures(&[signature, second_signature]).unwrap();

    assert_eq!(
        public_key,
        decode_array(
            "954087aafacc1046c0f0ad35d5b60163cb4771573f995afdd6f26cbeec117caaef1a94eed091f06cfbb04cd44819a4b419629b06ca5701c0c4a53b370db40a5adf174a8627ff0fe765eddfb0e4bb5debddcb7a268afec33c833f7f9466fded0c"
        )
    );
    assert_eq!(
        signature,
        decode_array(
            "8a0843ce5187848624a00a86ce657782def22e8ed59046a1723e0db715a018314d4a5982ac9abb5b8cbbd270f448ba0b"
        )
    );
    assert_eq!(
        aggregated,
        decode_array(
            "abf312ecc4e8c7d1c5acc41147a028cfb4d225abdb09ab0fe5d8bf98f1290328633824cb50768bbaebd1e064c9ad8c66"
        )
    );
    assert_eq!(
        Bls12381Crypto::aggregate_signatures(&[signature]).unwrap(),
        signature
    );
    assert!(Bls12381Crypto::verify(MESSAGE, &signature, &public_key).unwrap());
    assert!(
        Bls12381Crypto::verify_aggregated(MESSAGE, &aggregated, &[public_key, second_public_key],)
            .unwrap()
    );
}

#[test]
fn generated_private_keys_are_valid_for_signing() {
    for index in 0..128 {
        let private_key = *Bls12381Crypto::generate_private_key();
        let public_key = Bls12381Crypto::derive_public_key(&private_key)
            .unwrap_or_else(|error| panic!("generated key {index} should derive: {error}"));
        let signature = Bls12381Crypto::sign(MESSAGE, &private_key)
            .unwrap_or_else(|error| panic!("generated key {index} should sign: {error}"));

        assert!(
            Bls12381Crypto::verify(MESSAGE, &signature, &public_key)
                .unwrap_or_else(|error| panic!("generated key {index} should verify: {error}")),
            "generated key {index} signature should verify"
        );
    }
}

#[test]
fn invalid_private_key_scalars_are_rejected() {
    assert!(Bls12381Crypto::derive_public_key(&[0u8; 32]).is_err());
    assert!(Bls12381Crypto::derive_public_key(&[0xffu8; 32]).is_err());
}
