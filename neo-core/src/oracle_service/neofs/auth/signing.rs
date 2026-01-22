use crate::neo_io::BinaryWriter;
use crate::wallets::KeyPair;
use base64::Engine as _;
use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::signature::Signer as P256Signer;
use p256::ecdsa::{Signature as P256Signature, SigningKey as P256SigningKey};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha512};

pub(crate) fn sign_neofs_bearer(
    token: &str,
    key: &KeyPair,
    wallet_connect: bool,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(token)
        .ok()?;
    if data.is_empty() {
        return None;
    }
    let signature = if wallet_connect {
        sign_neofs_wallet_connect(&data, key).ok()?
    } else {
        sign_neofs_sha512(&data, key).ok()?
    };
    Some((signature, key.compressed_public_key()))
}

pub(crate) fn sign_neofs_sha512(data: &[u8], key: &KeyPair) -> Result<Vec<u8>, String> {
    let signing_key = P256SigningKey::from_bytes(key.private_key().into())
        .map_err(|err| format!("invalid neofs key: {err}"))?;
    let digest = Sha512::digest(data);
    let signature: P256Signature = signing_key
        .sign_prehash(&digest)
        .map_err(|err| format!("failed to sign bearer token: {err}"))?;
    let sig_bytes = signature.to_bytes();
    let mut output = Vec::with_capacity(1 + sig_bytes.len());
    output.push(0x04);
    output.extend_from_slice(&sig_bytes);
    Ok(output)
}

fn sign_neofs_wallet_connect(data: &[u8], key: &KeyPair) -> Result<Vec<u8>, String> {
    let signing_key = P256SigningKey::from_bytes(key.private_key().into())
        .map_err(|err| format!("invalid neofs key: {err}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let message = salt_message_wallet_connect(b64.as_bytes(), &salt);
    let signature: P256Signature = signing_key.sign(message.as_slice());
    let mut output = signature.to_bytes().to_vec();
    output.extend_from_slice(&salt);
    Ok(output)
}

pub(crate) fn salt_message_wallet_connect(data: &[u8], salt: &[u8; 16]) -> Vec<u8> {
    let salt_hex = hex::encode(salt);
    let salted_len = salt_hex.len() + data.len();
    let mut writer = BinaryWriter::new();
    writer
        .write_bytes(&[0x01, 0x00, 0x01, 0xf0])
        .expect("write prefix");
    writer
        .write_var_uint(salted_len as u64)
        .expect("write length");
    writer.write_bytes(salt_hex.as_bytes()).expect("write salt");
    writer.write_bytes(data).expect("write data");
    writer.write_bytes(&[0x00, 0x00]).expect("write suffix");
    writer.into_bytes()
}
