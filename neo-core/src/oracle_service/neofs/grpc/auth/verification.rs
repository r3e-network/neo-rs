use super::super::super::auth::sign_neofs_sha512;
use super::super::super::proto::neofs_v2;
use crate::wallets::KeyPair;
use prost::Message;

pub(crate) fn build_neofs_request_verification_header<B: Message>(
    body: &B,
    meta: &neofs_v2::session::RequestMetaHeader,
    key: &KeyPair,
) -> Result<neofs_v2::session::RequestVerificationHeader, String> {
    let body_signature = neofs_sign_message_part(&body.encode_to_vec(), key)?;
    let meta_signature = neofs_sign_message_part(&meta.encode_to_vec(), key)?;
    let origin_signature = neofs_sign_message_part(&[], key)?;
    Ok(neofs_v2::session::RequestVerificationHeader {
        body_signature: Some(body_signature),
        meta_signature: Some(meta_signature),
        origin_signature: Some(origin_signature),
        origin: None,
    })
}

fn neofs_sign_message_part(
    data: &[u8],
    key: &KeyPair,
) -> Result<neofs_v2::refs::Signature, String> {
    let signature = sign_neofs_sha512(data, key)?;
    Ok(neofs_v2::refs::Signature {
        key: key.compressed_public_key(),
        sign: signature,
        scheme: neofs_v2::refs::SignatureScheme::EcdsaSha512 as i32,
    })
}
