use super::*;
use neo_primitives::HASH_SIZE;

#[test]
fn test_key_pair_generation() {
    let key_pair = KeyPair::generate().unwrap();
    assert_eq!(key_pair.private_key().len(), HASH_SIZE);
    assert!(!key_pair.public_key().is_empty());
    assert!(!key_pair.compressed_public_key().is_empty());
}

#[test]
fn nep2_uses_canonical_checksig_verification_script() {
    // Regression: the NEP-2 salt script must be the canonical CheckSig
    // verification script (PUSHDATA1 + 33-byte pubkey + SYSCALL + CheckSig
    // hash = 40 bytes), matching C# Contract.CreateSignatureRedeemScript —
    // not the old 62-byte raw-ASCII "System.Crypto.CheckWitness" form that
    // made '6P…' keys non-interoperable with standard wallets.
    let pk = [1u8; HASH_SIZE];
    let kp = KeyPair::from_private_key(&pk).unwrap();
    let script = KeyPair::try_get_verification_script_for_key(&pk).unwrap();
    assert_eq!(script, kp.verification_script());
    assert_eq!(script.len(), 40, "canonical CheckSig script is 40 bytes");

    // A standard '6P…' NEP-2 string round-trips at the N3 address version.
    let nep2 = kp.to_nep2("Satoshi", 0x35).unwrap();
    assert!(nep2.starts_with("6P"));
    let restored = KeyPair::from_nep2_string(&nep2, "Satoshi", 0x35).unwrap();
    assert_eq!(restored.private_key(), &pk);
}

#[test]
fn test_wif_round_trip() {
    let key_pair = KeyPair::generate().unwrap();
    let wif = key_pair.to_wif();
    let restored = KeyPair::from_wif(&wif).unwrap();
    assert_eq!(key_pair.private_key(), restored.private_key());
}

#[test]
fn test_sign_verify() {
    let key_pair = KeyPair::generate().unwrap();
    let data = b"test data";
    let signature = key_pair.sign(data).unwrap();
    assert!(key_pair.verify(data, &signature).unwrap());
}

/// NEP-2 export must use Base58Check (standard "6P..." prefix), and AES-256-ECB,
/// matching C# KeyPair.Encrypt. A base64-encoded or CBC-encrypted key would not
/// be importable by C#/standard wallets. Round-trip + standard-prefix guard.
#[test]
fn test_nep2_round_trip_uses_standard_base58check_format() {
    // N3 address version (0x35).
    const N3_ADDRESS_VERSION: u8 = 0x35;
    let key_pair = KeyPair::generate().unwrap();
    let password = "Satoshi";

    let nep2 = key_pair.to_nep2(password, N3_ADDRESS_VERSION).unwrap();
    // NEP-2 (prefix bytes 0x01 0x42) Base58Check-encodes to a "6P..." string.
    // A base64 encoding would never start with "6P".
    assert!(
        nep2.starts_with("6P"),
        "NEP-2 string must be Base58Check ('6P...'), got: {nep2}"
    );

    let restored = KeyPair::from_nep2_string(&nep2, password, N3_ADDRESS_VERSION).unwrap();
    assert_eq!(key_pair.private_key(), restored.private_key());

    // Wrong password must fail the address-hash verification.
    assert!(KeyPair::from_nep2_string(&nep2, "wrong", N3_ADDRESS_VERSION).is_err());
}

/// A NEP-6 wallet carries its own `ScryptParameters`; a key encrypted with
/// non-default parameters must round-trip only with the SAME parameters, and the
/// NEP-6 default (16384/8/8) must reject it — matching C# `NEP6Account`, which
/// threads `wallet.Scrypt.N/R/P` into encrypt/decrypt.
#[test]
fn nep2_honors_non_default_scrypt_parameters() {
    const N3_ADDRESS_VERSION: u8 = 0x35;
    let key_pair = KeyPair::generate().unwrap();
    let password = "Satoshi";
    // Non-default, but valid, scrypt cost (N must be a power of two).
    let (n, r, p) = (4096u32, 8u32, 8u32);

    let nep2 = key_pair
        .to_nep2_with_params(password, N3_ADDRESS_VERSION, n, r, p)
        .unwrap();
    assert!(nep2.starts_with("6P"));

    // Same parameters -> recovers the key.
    let restored =
        KeyPair::from_nep2_string_with_params(&nep2, password, N3_ADDRESS_VERSION, n, r, p)
            .unwrap();
    assert_eq!(key_pair.private_key(), restored.private_key());

    // Default parameters (16384/8/8) must NOT recover a key encrypted at N=4096:
    // scrypt derives a different key stream, so the address-hash check fails.
    assert!(
        KeyPair::from_nep2_string(&nep2, password, N3_ADDRESS_VERSION).is_err(),
        "default scrypt params must reject a non-default-encrypted NEP-2 key"
    );
}
