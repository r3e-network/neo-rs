use super::*;

#[test]
fn wallet_connect_signature_uses_shared_p256_signer() {
    let key = KeyPair::from_private_key(&[7u8; 32]).expect("test key");
    let data = b"wallet-connect payload";

    let output =
        NeoFsBearerSigner::sign_neofs_wallet_connect(data, &key).expect("wallet connect signature");

    assert_eq!(output.len(), 80);
    let signature: [u8; 64] = output[..64].try_into().expect("signature length");
    let salt: [u8; 16] = output[64..].try_into().expect("salt length");
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let message = NeoFsBearerSigner::salt_message_wallet_connect(b64.as_bytes(), &salt);

    assert!(
        Secp256r1Crypto::verify(&message, &signature, &key.compressed_public_key())
            .expect("wallet connect signature verification")
    );
}
