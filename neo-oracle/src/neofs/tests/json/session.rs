#[cfg(feature = "neofs-grpc")]
use super::super::super::json::neofs_json_session_token;
#[cfg(feature = "neofs-grpc")]
use super::super::super::proto::neofs_v2;
#[cfg(feature = "neofs-grpc")]
use base64::Engine as _;

#[cfg(feature = "neofs-grpc")]
#[test]
fn neofs_json_session_token_matches_csharp_format() {
    let id_bytes = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let owner_bytes = vec![2u8; 25];
    let session_key = vec![3u8; 33];
    let container_bytes = vec![4u8; 32];
    let object_bytes = vec![5u8; 32];
    let key_bytes = vec![6u8; 33];
    let sign_bytes = vec![7u8; 65];

    let id_b64 = base64::engine::general_purpose::STANDARD.encode(&id_bytes);
    let owner_b64 = base64::engine::general_purpose::STANDARD.encode(&owner_bytes);
    let session_b64 = base64::engine::general_purpose::STANDARD.encode(&session_key);
    let container_b64 = base64::engine::general_purpose::STANDARD.encode(&container_bytes);
    let object_b64 = base64::engine::general_purpose::STANDARD.encode(&object_bytes);
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&key_bytes);
    let sign_b64 = base64::engine::general_purpose::STANDARD.encode(&sign_bytes);

    let token = neofs_v2::session::SessionToken {
        body: Some(neofs_v2::session::session_token::Body {
            id: id_bytes,
            owner_id: Some(neofs_v2::refs::OwnerId { value: owner_bytes }),
            lifetime: Some(neofs_v2::session::session_token::body::TokenLifetime {
                exp: 12,
                nbf: 34,
                iat: 56,
            }),
            session_key,
            context: Some(neofs_v2::session::session_token::body::Context::Object(
                neofs_v2::session::ObjectSessionContext {
                    verb: neofs_v2::session::object_session_context::Verb::Get as i32,
                    target: Some(neofs_v2::session::object_session_context::Target {
                        container: Some(neofs_v2::refs::ContainerId {
                            value: container_bytes,
                        }),
                        objects: vec![neofs_v2::refs::ObjectId {
                            value: object_bytes,
                        }],
                    }),
                },
            )),
        }),
        signature: Some(neofs_v2::refs::Signature {
            key: key_bytes,
            sign: sign_bytes,
            scheme: neofs_v2::refs::SignatureScheme::EcdsaSha512 as i32,
        }),
    };

    let json = neofs_json_session_token(&token).expect("token json");
    // Note: Rust serializes lifetime values as strings to match Neo's JSON format
    let expected = format!(
        "{{ \"body\": {{ \"id\": \"{id_b64}\", \"ownerID\": {{ \"value\": \"{owner_b64}\" }}, \
 \"lifetime\": {{ \"exp\": \"12\", \"nbf\": \"34\", \"iat\": \"56\" }}, \
 \"sessionKey\": \"{session_b64}\", \
 \"object\": {{ \"verb\": \"GET\", \"address\": {{ \"containerID\": {{ \"value\": \"{container_b64}\" }}, \
 \"objectID\": {{ \"value\": \"{object_b64}\" }} }} }} }}, \
 \"signature\": {{ \"key\": \"{key_b64}\", \"signature\": \"{sign_b64}\", \"scheme\": \"ECDSA_SHA512\" }} }}"
    );
    assert_eq!(json, expected);
}
