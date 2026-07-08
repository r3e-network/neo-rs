use super::super::proto::neofs_v2;
use neo_crypto::{NEOFS_ECDSA_SHA512_SIGNATURE_LEN, Secp256r1Crypto};
use neo_error::{CoreError, CoreResult};
use prost::Message;

pub(super) fn validate_neofs_response<B: Message>(
    body: &B,
    meta: Option<&neofs_v2::session::ResponseMetaHeader>,
    verify: Option<&neofs_v2::session::ResponseVerificationHeader>,
) -> CoreResult<()> {
    let meta = meta.ok_or_else(|| CoreError::other("missing meta header"))?;
    let verify = verify.ok_or_else(|| CoreError::other("missing verify header"))?;
    if !verify_neofs_matryoshka(body, meta, verify) {
        return Err(CoreError::other("invalid neofs response signature"));
    }
    if let Some(status) = meta.status.as_ref() {
        if !is_neofs_status_success(status) {
            return Err(CoreError::other("neofs response status error"));
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

    let Some(origin_verify) = verify.origin.as_ref() else {
        let body_sig = match verify.body_signature.as_ref() {
            Some(sig) => sig,
            None => return false,
        };
        return verify_neofs_signature_bytes(body_sig, &body.encode_to_vec());
    };

    if verify.body_signature.is_some() {
        return false;
    }
    let Some(origin_meta) = meta.origin.as_ref() else {
        return false;
    };
    verify_neofs_matryoshka(body, origin_meta, origin_verify)
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
#[path = "../../tests/neofs/grpc/verify.rs"]
mod tests;
