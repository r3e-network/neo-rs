use super::super::proto::neofs_v2;
use neo_crypto::{Secp256r1Crypto, NEOFS_ECDSA_SHA512_SIGNATURE_LEN};
use prost::Message;

pub(super) fn validate_neofs_response<B: Message>(
    body: &B,
    meta: Option<&neofs_v2::session::ResponseMetaHeader>,
    verify: Option<&neofs_v2::session::ResponseVerificationHeader>,
) -> Result<(), String> {
    let meta = meta.ok_or_else(|| "missing meta header".to_string())?;
    let verify = verify.ok_or_else(|| "missing verify header".to_string())?;
    if !verify_neofs_matryoshka(body, meta, verify) {
        return Err("invalid neofs response signature".to_string());
    }
    if let Some(status) = meta.status.as_ref() {
        if !is_neofs_status_success(status) {
            return Err("neofs response status error".to_string());
        }
    }
    Ok(())
}

fn verify_neofs_matryoshka<B: Message>(
    body: &B,
    meta: &neofs_v2::session::ResponseMetaHeader,
    verify: &neofs_v2::session::ResponseVerificationHeader,
) -> bool {
    let meta_sig = match verify.meta_signature.as_ref() {
        Some(sig) => sig,
        None => return false,
    };
    if !verify_neofs_signature_bytes(meta_sig, &meta.encode_to_vec()) {
        return false;
    }

    let origin_bytes = verify
        .origin
        .as_ref()
        .map(|origin| origin.encode_to_vec())
        .unwrap_or_default();
    let origin_sig = match verify.origin_signature.as_ref() {
        Some(sig) => sig,
        None => return false,
    };
    if !verify_neofs_signature_bytes(origin_sig, &origin_bytes) {
        return false;
    }

    if verify.origin.is_none() {
        let body_sig = match verify.body_signature.as_ref() {
            Some(sig) => sig,
            None => return false,
        };
        return verify_neofs_signature_bytes(body_sig, &body.encode_to_vec());
    }

    if verify.body_signature.is_some() {
        return false;
    }
    let Some(origin_meta) = meta.origin.as_ref() else {
        return false;
    };
    verify_neofs_matryoshka(body, origin_meta, verify.origin.as_ref().expect("origin"))
}

pub(super) fn verify_neofs_signature_bytes(
    signature: &neofs_v2::refs::Signature,
    data: &[u8],
) -> bool {
    if signature.key.is_empty() || signature.sign.len() != NEOFS_ECDSA_SHA512_SIGNATURE_LEN {
        return false;
    }
    Secp256r1Crypto::verify_neofs_sha512(data, &signature.sign, &signature.key).unwrap_or(false)
}

fn is_neofs_status_success(status: &neofs_v2::status::Status) -> bool {
    status.code < 1024
}

#[cfg(test)]
mod tests {
    use super::{neofs_v2, verify_neofs_signature_bytes};
    use neo_crypto::Secp256r1Crypto;
    use crate::neofs::auth::sign_neofs_sha512;
    use neo_wallets::KeyPair;

    #[test]
    fn verifies_neofs_signature_from_core_signing_path() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let key = KeyPair::from_private_key(&private_key).expect("test key");
        let data = b"neofs response body";
        let signature = neofs_v2::refs::Signature {
            key: key.compressed_public_key(),
            sign: sign_neofs_sha512(data, &key).expect("neofs signature"),
            scheme: neofs_v2::refs::SignatureScheme::EcdsaSha512 as i32,
        };

        assert!(verify_neofs_signature_bytes(&signature, data));
        assert!(!verify_neofs_signature_bytes(&signature, b"mutated"));
    }
}
