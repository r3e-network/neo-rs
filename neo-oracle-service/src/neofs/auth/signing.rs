use neo_crypto::Secp256r1Crypto;
use neo_core::neo_io::BinaryWriter;
use neo_core::wallets::KeyPair;
use base64::Engine as _;
use rand::RngCore;
use rand::rngs::OsRng;

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
    Secp256r1Crypto::sign_neofs_sha512(data, key.private_key())
        .map(|signature| signature.to_vec())
        .map_err(|err| format!("failed to sign bearer token: {err}"))
}

fn sign_neofs_wallet_connect(data: &[u8], key: &KeyPair) -> Result<Vec<u8>, String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let message = salt_message_wallet_connect(b64.as_bytes(), &salt);
    let signature = Secp256r1Crypto::sign(&message, key.private_key())
        .map_err(|err| format!("invalid neofs key: {err}"))?;
    let mut output = signature.to_vec();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_connect_signature_uses_shared_p256_signer() {
        let key = KeyPair::from_private_key(&[7u8; 32]).expect("test key");
        let data = b"wallet-connect payload";

        let output = sign_neofs_wallet_connect(data, &key).expect("wallet connect signature");

        assert_eq!(output.len(), 80);
        let signature: [u8; 64] = output[..64].try_into().expect("signature length");
        let salt: [u8; 16] = output[64..].try_into().expect("salt length");
        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
        let message = salt_message_wallet_connect(b64.as_bytes(), &salt);

        assert!(
            Secp256r1Crypto::verify(&message, &signature, &key.compressed_public_key())
                .expect("wallet connect signature verification")
        );
    }
}
