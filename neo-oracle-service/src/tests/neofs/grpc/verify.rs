use super::{neofs_v2, verify_neofs_signature_bytes};
use crate::neofs::auth::NeoFsBearerSigner;
use neo_crypto::Secp256r1Crypto;
use neo_wallets::KeyPair;

#[test]
fn verifies_neofs_signature_from_core_signing_path() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let key = KeyPair::from_private_key(&private_key).expect("test key");
    let data = b"neofs response body";
    let signature = neofs_v2::refs::Signature {
        key: key.compressed_public_key(),
        sign: NeoFsBearerSigner::sign_neofs_sha512(data, &key).expect("neofs signature"),
        scheme: neofs_v2::refs::SignatureScheme::EcdsaSha512 as i32,
    };

    assert!(verify_neofs_signature_bytes(&signature, data));
    assert!(!verify_neofs_signature_bytes(&signature, b"mutated"));
}
