use base64::Engine as _;
use neo_crypto::Secp256r1Crypto;
use neo_error::{CoreError, CoreResult};
use neo_io::var_int::VarInt;
use neo_primitives::hex_util;
use neo_wallets::KeyPair;
use rand::RngCore;
use rand::rngs::OsRng;

/// NeoFS bearer-token signing helpers.
pub(crate) struct NeoFsBearerSigner;

impl NeoFsBearerSigner {
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
            NeoFsBearerSigner::sign_neofs_wallet_connect(&data, key).ok()?
        } else {
            NeoFsBearerSigner::sign_neofs_sha512(&data, key).ok()?
        };
        Some((signature, key.compressed_public_key()))
    }

    pub(crate) fn sign_neofs_sha512(data: &[u8], key: &KeyPair) -> CoreResult<Vec<u8>> {
        Secp256r1Crypto::sign_neofs_sha512(data, key.private_key())
            .map(|signature| signature.to_vec())
            .map_err(|err| CoreError::other(format!("failed to sign bearer token: {err}")))
    }

    fn sign_neofs_wallet_connect(data: &[u8], key: &KeyPair) -> CoreResult<Vec<u8>> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let message = NeoFsBearerSigner::salt_message_wallet_connect(b64.as_bytes(), &salt);
        let signature = Secp256r1Crypto::sign(&message, key.private_key())
            .map_err(|err| CoreError::other(format!("invalid neofs key: {err}")))?;
        let mut output = signature.to_vec();
        output.extend_from_slice(&salt);
        Ok(output)
    }

    pub(crate) fn salt_message_wallet_connect(data: &[u8], salt: &[u8; 16]) -> Vec<u8> {
        let salt_hex = hex_util::encode_hex(salt);
        let salted_len = salt_hex.len() + data.len();
        let mut message =
            Vec::with_capacity(4 + VarInt::encoded_len(salted_len as u64) + salted_len + 2);
        message.extend_from_slice(&[0x01, 0x00, 0x01, 0xf0]);
        VarInt::write_var_int(salted_len as u64, &mut message);
        message.extend_from_slice(salt_hex.as_bytes());
        message.extend_from_slice(data);
        message.extend_from_slice(&[0x00, 0x00]);
        message
    }
}

#[cfg(test)]
#[path = "../../tests/neofs/auth/signing.rs"]
mod tests;
