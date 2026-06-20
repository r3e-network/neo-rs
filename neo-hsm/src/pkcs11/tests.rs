use super::*;

#[test]
fn decode_der_octet_string_wrapped_and_bare() {
    let point = vec![0x04u8; 65];

    let mut wrapped = vec![0x04, 0x41];
    wrapped.extend_from_slice(&point);
    let decoded = decode_der_octet_string(&wrapped).unwrap();
    assert_eq!(decoded, point);

    let decoded_bare = decode_der_octet_string(&point).unwrap();
    assert_eq!(decoded_bare, point);
}

#[test]
fn normalize_to_compressed_roundtrip() {
    let mut uncompressed = [0u8; 65];
    uncompressed[0] = 0x04;
    uncompressed[64] = 0x00;
    let c = normalize_to_compressed(&uncompressed).unwrap();
    assert_eq!(c[0], 0x02);
    assert_eq!(&c[1..], &uncompressed[1..33]);

    let mut uncompressed2 = [0u8; 65];
    uncompressed2[0] = 0x04;
    uncompressed2[64] = 0x01;
    let c2 = normalize_to_compressed(&uncompressed2).unwrap();
    assert_eq!(c2[0], 0x03);

    let mut compressed = [0x02u8; 33];
    compressed[0] = 0x02;
    let c3 = normalize_to_compressed(&compressed).unwrap();
    assert_eq!(c3, compressed);
}

#[test]
fn signature_redeem_script_structure() {
    let pubkey = [0x02u8; 33];
    let script = signature_redeem_script(&pubkey);

    assert_eq!(script.len(), 40, "script must be 40 bytes");
    assert_eq!(script[0], 0x0C, "PUSHDATA1 opcode");
    assert_eq!(script[1], 0x21, "33-byte length prefix");
    assert_eq!(&script[2..35], pubkey.as_ref());
    assert_eq!(script[35], 0x41, "SYSCALL opcode");
}

#[test]
fn finalize_raw_rs_low_s() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let data = b"test signing data for neo hsm";
    let digest = Crypto::sha256(data);
    let raw: [u8; 64] = Secp256r1Crypto::sign_prehash(&digest, &private_key).unwrap();

    let result = finalize_signature(&raw, SigFormat::RawRs).unwrap();
    assert_eq!(result.len(), 64);

    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    use p256::{
        PublicKey as P256PublicKey,
        ecdsa::{Signature as P256Sig, VerifyingKey, signature::hazmat::PrehashVerifier},
    };
    let vk = VerifyingKey::from(P256PublicKey::from_sec1_bytes(&pubkey).unwrap());
    let sig_bytes: [u8; 64] = result.try_into().unwrap();
    let sig = P256Sig::from_slice(&sig_bytes).unwrap();
    assert!(vk.verify_prehash(&digest, &sig).is_ok());
}
