use super::super::proto::neofs_v2;
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature as P256Signature, VerifyingKey as P256VerifyingKey};
use prost::Message;
use sha2::{Digest, Sha512};

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
    if signature.key.is_empty() || signature.sign.len() != 65 {
        return false;
    }
    let verifying_key = match P256VerifyingKey::from_sec1_bytes(&signature.key) {
        Ok(key) => key,
        Err(_) => return false,
    };
    let sig = match P256Signature::from_slice(&signature.sign[1..]) {
        Ok(sig) => sig,
        Err(_) => return false,
    };
    let digest = Sha512::digest(data);
    verifying_key.verify_prehash(&digest, &sig).is_ok()
}

fn is_neofs_status_success(status: &neofs_v2::status::Status) -> bool {
    status.code < 1024
}
