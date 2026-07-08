//! # neo-native-contracts::tests::crypto_lib
//!
//! Test module grouping Native CryptoLib interop surface and verification
//! helpers. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use neo_config::Hardfork;
use neo_crypto::murmur;
use neo_primitives::ContractParameterType;
use num_bigint::BigInt;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn hash_methods_match_csharp_vectors() {
    // C# CryptoLib.{Sha256,RIPEMD160,Keccak256}(utf8("abc")).
    assert_eq!(
        hex(&CryptoLib::sha256_method(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&CryptoLib::ripemd160_method(b"abc")),
        "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
    );
    assert_eq!(
        hex(&CryptoLib::keccak256_method(b"abc")),
        "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
    );
}

#[test]
fn murmur32_is_little_endian() {
    // MurmurHash3 x86 32 of empty input with seed 0 is 0 -> LE bytes 0,0,0,0
    // (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
    assert_eq!(
        murmur::murmur32(b"", 0).to_le_bytes().to_vec(),
        vec![0u8, 0, 0, 0]
    );
    // Deterministic and non-trivial for a non-empty input.
    let h = murmur::murmur32(b"hello", 0);
    assert_eq!(murmur::murmur32(b"hello", 0), h);
    assert_eq!(h.to_le_bytes().len(), 4);
}

#[test]
fn murmur32_seed_is_strict_uint() {
    let max_seed = BigInt::from(u32::MAX).to_signed_bytes_le();
    assert_eq!(
        CryptoLib::murmur32_method(b"hello", &max_seed).unwrap(),
        murmur::murmur32(b"hello", u32::MAX).to_le_bytes()
    );

    let negative = BigInt::from(-1).to_signed_bytes_le();
    assert!(CryptoLib::murmur32_method(b"hello", &negative).is_err());

    let too_large = BigInt::from(u64::from(u32::MAX) + 1).to_signed_bytes_le();
    assert!(CryptoLib::murmur32_method(b"hello", &too_large).is_err());
}

#[test]
fn native_contract_surface_is_consistent() {
    let c = CryptoLib::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "sha256",
            "ripemd160",
            "keccak256",
            "murmur32",
            "verifyWithECDsa",   // V2 (ActiveIn Gorgon)
            "verifyWithEd25519", // V1 (ActiveIn Gorgon)
            "verifyWithEd25519", // V0 (ActiveIn Echidna, DeprecatedIn Gorgon)
            "verifyWithECDsa",   // V0 (genesis, DeprecatedIn Cockatrice)
            "verifyWithECDsa",   // V1 (ActiveIn Cockatrice, DeprecatedIn Gorgon)
            "recoverSecp256K1",
            "bls12381Serialize",
            "bls12381Deserialize",
            "bls12381Equal",
            "bls12381Add",
            "bls12381Mul",
            "bls12381Pairing",
        ]
    );
    // keccak256 is hardfork-gated; the unconditional hashes are not.
    let keccak = c.methods().iter().find(|m| m.name == "keccak256").unwrap();
    assert_eq!(keccak.active_in, Some(Hardfork::HfCockatrice));
    assert!(c.methods().iter().all(|m| m.safe));
    // The hashes/murmur return ByteArray; verifyWithEd25519 is a Gorgon
    // V1 plus Echidna V0 Boolean pair with three byte-array parameters.
    let ed: Vec<&NativeMethod> = c
        .methods()
        .iter()
        .filter(|m| m.name == "verifyWithEd25519")
        .collect();
    assert_eq!(ed.len(), 2);
    assert_eq!(ed[0].active_in, Some(Hardfork::HfGorgon));
    assert_eq!(ed[0].deprecated_in, None);
    assert_eq!(ed[1].active_in, Some(Hardfork::HfEchidna));
    assert_eq!(ed[1].deprecated_in, Some(Hardfork::HfGorgon));
    for method in &ed {
        assert_eq!(method.return_type, ContractParameterType::Boolean);
        assert_eq!(method.parameters.len(), 3);
    }
    // verifyWithECDsa is a triple registration (C# v3.10.1 V0/V1/V2): V0
    // runs from genesis until DeprecatedIn HF_Cockatrice with the fourth
    // parameter named `curve`; V1 is ActiveIn HF_Cockatrice and DeprecatedIn
    // HF_Gorgon; V2 is ActiveIn HF_Gorgon. Types are identical across versions.
    let ecdsa: Vec<&NativeMethod> = c
        .methods()
        .iter()
        .filter(|m| m.name == "verifyWithECDsa")
        .collect();
    assert_eq!(ecdsa.len(), 3);
    let (v2, v0, v1) = (ecdsa[0], ecdsa[1], ecdsa[2]);
    assert_eq!(v2.active_in, Some(Hardfork::HfGorgon));
    assert_eq!(v2.deprecated_in, None);
    assert_eq!(
        v2.parameter_names,
        ["message", "pubkey", "signature", "curveHash"]
    );
    assert_eq!(v0.active_in, None);
    assert_eq!(v0.deprecated_in, Some(Hardfork::HfCockatrice));
    assert_eq!(
        v0.parameter_names,
        ["message", "pubkey", "signature", "curve"]
    );
    assert_eq!(v1.active_in, Some(Hardfork::HfCockatrice));
    assert_eq!(v1.deprecated_in, Some(Hardfork::HfGorgon));
    assert_eq!(
        v1.parameter_names,
        ["message", "pubkey", "signature", "curveHash"]
    );
    for m in &ecdsa {
        assert_eq!(m.return_type, ContractParameterType::Boolean);
        assert_eq!(m.parameters.len(), 4);
        assert_eq!(m.parameters[3], ContractParameterType::Integer);
    }
    // recoverSecp256K1 is HF_Echidna-gated, safe, (messageHash, signature) ->
    // ByteArray (nullable at runtime via set_native_return_null).
    let recover = c
        .methods()
        .iter()
        .find(|m| m.name == "recoverSecp256K1")
        .unwrap();
    assert_eq!(recover.active_in, Some(Hardfork::HfEchidna));
    assert_eq!(recover.return_type, ContractParameterType::ByteArray);
    assert_eq!(
        recover.parameters,
        vec![ContractParameterType::ByteArray; 2]
    );
    assert!(recover.safe);

    // BLS12-381 ABI (genesis-active, all safe; CryptoLib.BLS12_381.cs fees).
    let interop = ContractParameterType::InteropInterface;
    let bls = |name: &str| {
        c.methods()
            .iter()
            .find(|m| m.name == name)
            .cloned()
            .unwrap()
    };
    let ser = bls("bls12381Serialize");
    assert_eq!(ser.cpu_fee, 1 << 19);
    assert_eq!(ser.parameters, vec![interop]);
    assert_eq!(ser.return_type, ContractParameterType::ByteArray);
    let de = bls("bls12381Deserialize");
    assert_eq!(de.cpu_fee, 1 << 19);
    assert_eq!(de.parameters, vec![ContractParameterType::ByteArray]);
    assert_eq!(de.return_type, interop);
    let eq = bls("bls12381Equal");
    assert_eq!(eq.cpu_fee, 1 << 5);
    assert_eq!(eq.parameters, vec![interop, interop]);
    assert_eq!(eq.return_type, ContractParameterType::Boolean);
    let add = bls("bls12381Add");
    assert_eq!(add.cpu_fee, 1 << 19);
    assert_eq!(add.parameters, vec![interop, interop]);
    assert_eq!(add.return_type, interop);
    let mul = bls("bls12381Mul");
    assert_eq!(mul.cpu_fee, 1 << 21);
    assert_eq!(
        mul.parameters,
        vec![
            interop,
            ContractParameterType::ByteArray,
            ContractParameterType::Boolean
        ]
    );
    assert_eq!(mul.return_type, interop);
    let pairing = bls("bls12381Pairing");
    assert_eq!(pairing.cpu_fee, 1 << 23);
    assert_eq!(pairing.parameters, vec![interop, interop]);
    assert_eq!(pairing.return_type, interop);
    for name in [
        "bls12381Serialize",
        "bls12381Deserialize",
        "bls12381Equal",
        "bls12381Add",
        "bls12381Mul",
        "bls12381Pairing",
    ] {
        let m = bls(name);
        assert!(m.safe, "{name} is safe");
        assert_eq!(m.active_in, None, "{name} is genesis-active");
    }
}

// BLS12-381 dispatch vectors (a subset of UT_CryptoLib; s_gtHex == e(g1,g2)).
// The full byte-exact arithmetic is verified in neo_crypto::bls12381_point —
// these confirm the native dispatch maps each method to the right operation,
// parses the (point, scalar, neg) arguments correctly, and returns canonical
// bytes / boolean bytes the way the engine marshaling expects.
const BLS_G1: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb";
const BLS_G2: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";
const BLS_GT: &str = "0f41e58663bf08cf068672cbd01a7ec73baca4d72ca93544deff686bfd6df543d48eaa24afe47e1efde449383b67663104c581234d086a9902249b64728ffd21a189e87935a954051c7cdba7b3872629a4fafc05066245cb9108f0242d0fe3ef03350f55a7aefcd3c31b4fcb6ce5771cc6a0e9786ab5973320c806ad360829107ba810c5a09ffdd9be2291a0c25a99a211b8b424cd48bf38fcef68083b0b0ec5c81a93b330ee1a677d0d15ff7b984e8978ef48881e32fac91b93b47333e2ba5706fba23eb7c5af0d9f80940ca771b6ffd5857baaf222eb95a7d2809d61bfe02e1bfd1b68ff02f0b8102ae1c2d5d5ab1a19f26337d205fb469cd6bd15c3d5a04dc88784fbb3d0b2dbdea54d43b2b73f2cbb12d58386a8703e0f948226e47ee89d018107154f25a764bd3c79937a45b84546da634b8f6be14a8061e55cceba478b23f7dacaa35c8ca78beae9624045b4b601b2f522473d171391125ba84dc4007cfbf2f8da752f7c74185203fcca589ac719c34dffbbaad8431dad1c1fb597aaa5193502b86edb8857c273fa075a50512937e0794e1e65a7617c90d8bd66065b1fffe51d7a579973b1315021ec3c19934f1368bb445c7c2d209703f239689ce34c0378a68e72a6b3b216da0e22a5031b54ddff57309396b38c881c4c849ec23e87089a1c5b46e5110b86750ec6a532348868a84045483c92b7af5af689452eafabf1a8943e50439f1d59882a98eaa0170f1250ebd871fc0a92a7b2d83168d0d727272d441befa15c503dd8e90ce98db3e7b6d194f60839c508a84305aaca1789b6";

#[test]
fn bls12381_dispatch_matches_crypto_layer() {
    let g1 = hex::decode(BLS_G1).unwrap();
    let g2 = hex::decode(BLS_G2).unwrap();
    let gt = hex::decode(BLS_GT).unwrap();
    let scalar = |n: u8| {
        let mut s = [0u8; 32];
        s[0] = n;
        s.to_vec()
    };

    // Deserialize normalizes to canonical bytes; Serialize returns them.
    assert_eq!(
        CryptoLib::bls12381_deserialize_method(std::slice::from_ref(&g1)).unwrap(),
        g1
    );
    assert_eq!(
        CryptoLib::bls12381_serialize_method(std::slice::from_ref(&g1)).unwrap(),
        g1
    );

    // Pairing e(g1,g2) == s_gtHex — the headline C# vector through dispatch.
    assert_eq!(
        CryptoLib::bls12381_pairing_method(&[g1.clone(), g2.clone()]).unwrap(),
        gt
    );

    // Add(gt,gt) == Mul(gt, 2): cross-checks the Add and Mul wiring.
    assert_eq!(
        CryptoLib::bls12381_add_method(&[gt.clone(), gt.clone()]).unwrap(),
        CryptoLib::bls12381_mul_method(&[gt.clone(), scalar(2), vec![0]]).unwrap()
    );

    // gt*3 + gt*(-3) == gt*0 (identity): verifies Mul's `neg` flag + Add.
    let pos = CryptoLib::bls12381_mul_method(&[gt.clone(), scalar(3), vec![0]]).unwrap();
    let neg = CryptoLib::bls12381_mul_method(&[gt.clone(), scalar(3), vec![1]]).unwrap();
    let identity = CryptoLib::bls12381_mul_method(&[gt.clone(), scalar(0), vec![0]]).unwrap();
    assert_eq!(
        CryptoLib::bls12381_equal_method(&[
            CryptoLib::bls12381_add_method(&[pos, neg]).unwrap(),
            identity
        ])
        .unwrap(),
        vec![1u8]
    );

    // Equal: same point true; a cross-group comparison FAULTS in C#
    // (`ArgumentException("BLS12-381 type mismatch")`), not returns false.
    assert_eq!(
        CryptoLib::bls12381_equal_method(&[g1.clone(), g1.clone()]).unwrap(),
        vec![1u8]
    );
    assert!(
        CryptoLib::bls12381_equal_method(&[g1.clone(), g2.clone()]).is_err(),
        "cross-group bls12381Equal must fault like C#"
    );

    // Faults (Err -> VM fault): malformed point, swapped pairing operands,
    // wrong scalar length.
    assert!(CryptoLib::bls12381_deserialize_method(&[vec![0u8; 47]]).is_err());
    assert!(CryptoLib::bls12381_pairing_method(&[g2.clone(), g1.clone()]).is_err());
    assert!(CryptoLib::bls12381_mul_method(&[gt.clone(), vec![0u8; 31], vec![0]]).is_err());
}

#[test]
fn recover_secp256k1_returns_none_on_bad_input() {
    // The success path is round-trip-tested in neo-crypto
    // (recover_public_key_round_trips_and_rejects_bad_input); here we cover the
    // null path that maps to C# RecoverSecp256K1 returning null.
    let hash = [0x42u8; 32];
    assert!(CryptoLib::recover_secp256k1_method(&hash, &[0u8; 10]).is_none()); // bad sig length
    assert!(CryptoLib::recover_secp256k1_method(&[0u8; 31], &[0u8; 65]).is_none()); // bad hash length
    assert!(CryptoLib::recover_secp256k1_method(&hash, &[0u8; 65]).is_none()); // invalid signature
    // C# `Crypto.ECRecover` requires exactly 65 bytes → a 64-byte signature is
    // rejected (returns null).
    assert!(CryptoLib::recover_secp256k1_method(&hash, &[0u8; 64]).is_none()); // 64-byte rejected
}

#[test]
fn recover_secp256k1_rejects_64_byte_compact_signature_like_csharp() {
    // C# `Crypto.ECRecover` requires exactly 65 bytes (`r‖s‖v`) and throws
    // `ArgumentException` on any other length (`if (signature.Length != 65)
    // throw`). `RecoverSecp256K1` catches the exception and returns null. A
    // 64-byte EIP-2098 compact signature must also return null (None).
    let sk = [0x11u8; 32];
    let msg = b"neo ecrecover parity";
    let hash = neo_crypto::Crypto::sha256(msg);
    let expected = neo_crypto::Secp256k1Crypto::derive_public_key(&sk)
        .unwrap()
        .to_vec();
    let sig64 = neo_crypto::Secp256k1Crypto::sign(msg, &sk).unwrap();

    // The 65-byte form with the correct recovery id recovers the signer key and
    // is accepted by the consensus method.
    let mut matched = false;
    for v in 0u8..=3 {
        let mut sig65 = sig64.to_vec();
        sig65.push(v);
        if CryptoLib::recover_secp256k1_method(&hash, &sig65).as_deref()
            == Some(expected.as_slice())
        {
            matched = true;
            break;
        }
    }
    assert!(
        matched,
        "a 65-byte signature must still recover the signer key"
    );

    // The 64-byte compact form is REJECTED — C# `Crypto.ECRecover` throws on
    // non-65-byte signatures, and `RecoverSecp256K1` returns null.
    assert!(
        CryptoLib::recover_secp256k1_method(&hash, &sig64).is_none(),
        "a 64-byte compact signature must be rejected, matching C# ECRecover"
    );
}

#[test]
fn verify_ecdsa_dispatch_gates_keccak_and_rejects_unknown_curve() {
    // The curve/hash dispatch + Cockatrice gate are tested here; the ECDSA
    // mechanics themselves are covered by neo-crypto's verify_signature_with_hash
    // tests (SHA-256 cross-check + Keccak-256 round-trips).
    let msg = b"message";
    let empty = b"";
    let sk = [0x11u8; 32];
    // Valid keys for each curve, so a bad-length signature returns false (proving
    // the curve dispatched) rather than the KEY faulting on decode.
    let k1 = neo_crypto::Secp256k1Crypto::derive_public_key(&sk).unwrap();
    let r1 = neo_crypto::Secp256r1Crypto::derive_public_key(&sk).unwrap();
    let short = [0u8; 10]; // wrong-length sig -> false after a valid key decodes

    // Undefined curve byte -> error before any decode (C# KeyNotFound/
    // ArgumentOutOfRange faults).
    assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x00, true, false).is_err());

    // SHA-256 curves (0x16 secp256k1 / 0x17 secp256r1) are valid at any height; a
    // valid key + bad-length signature dispatches to a false result.
    assert!(!CryptoLib::verify_ecdsa_method(msg, &k1, &short, 0x16, false, false).unwrap());
    assert!(!CryptoLib::verify_ecdsa_method(msg, &r1, &short, 0x17, false, false).unwrap());

    // Keccak-256 curves (0x7A/0x7B) require Cockatrice: gated off -> fault (the
    // gate fires before any key decode).
    assert!(CryptoLib::verify_ecdsa_method(msg, &k1, &short, 0x7A, false, false).is_err());
    assert!(CryptoLib::verify_ecdsa_method(msg, &r1, &short, 0x7B, false, false).is_err());
    // Enabled -> dispatch (valid key + bad-length sig -> false).
    assert!(!CryptoLib::verify_ecdsa_method(msg, &k1, &short, 0x7A, true, false).unwrap());
    assert!(!CryptoLib::verify_ecdsa_method(msg, &r1, &short, 0x7B, true, false).unwrap());
}

#[test]
fn verify_ecdsa_gorgon_faults_on_bad_format_like_csharp_v2() {
    let msg = b"message";
    let empty = b"";

    // V1 (active v3.10.1): a malformed public key FAULTS. C# decodes the key
    // (`ECPoint.DecodePoint`) before the signature-length check, and its
    // `FormatException` is NOT caught by `catch(ArgumentException)` — so an empty
    // key faults even though the signature length is also wrong.
    assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x16, true, false).is_err());
    // V2 calls C# Crypto.VerifySignature, whose length/public-key checks fault.
    assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x16, true, true).is_err());
}

#[test]
fn verify_ecdsa_v1_good_key_bad_signature_returns_false_not_fault() {
    // A VALID public key with a wrong-length or non-verifying signature returns
    // false on the active V0/V1 path (C# `VerifySignatureV0` returns false after
    // the key decodes) — only a malformed KEY faults.
    let msg = b"message";
    let sk = [0x11u8; 32];
    let pubkey = neo_crypto::Secp256k1Crypto::derive_public_key(&sk).unwrap();
    // secp256k1SHA256 = 0x16. Wrong-length signature -> false (not a fault).
    let short_sig = [0u8; 10];
    assert!(
        !CryptoLib::verify_ecdsa_method(msg, &pubkey, &short_sig, 0x16, true, false).unwrap(),
        "good key + wrong-length signature must return false, not fault"
    );
    // A correct-length but non-verifying signature -> false.
    let zero_sig = [0u8; 64];
    assert!(
        !CryptoLib::verify_ecdsa_method(msg, &pubkey, &zero_sig, 0x16, true, false).unwrap(),
        "good key + non-verifying signature must return false"
    );
}

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn verify_ed25519_matches_rfc8032_test1() {
    // RFC 8032 Section 7.1, Test 1 (empty message).
    let pubkey = hex_bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let signature = hex_bytes(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    );
    let message: &[u8] = b"";
    assert!(CryptoLib::verify_ed25519_method(
        message, &pubkey, &signature
    ));

    // A tampered signature fails.
    let mut bad = signature.clone();
    bad[0] ^= 0x01;
    assert!(!CryptoLib::verify_ed25519_method(message, &pubkey, &bad));

    // Wrong-length inputs return false without panicking (C# length guards).
    assert!(!CryptoLib::verify_ed25519_method(
        message,
        &pubkey[..31],
        &signature
    ));
    assert!(!CryptoLib::verify_ed25519_method(
        message,
        &pubkey,
        &signature[..63]
    ));
}

#[test]
fn verify_ed25519_gorgon_faults_on_bad_lengths_like_csharp_v1() {
    let pubkey = hex_bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let signature = hex_bytes(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    );

    assert!(CryptoLib::verify_ed25519_gorgon_method(b"", &pubkey[..31], &signature).is_err());
    assert!(CryptoLib::verify_ed25519_gorgon_method(b"", &pubkey, &signature[..63]).is_err());
}
