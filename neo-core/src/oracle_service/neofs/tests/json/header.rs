#[cfg(feature = "neofs-grpc")]
use super::super::super::json::neofs_json_header;
#[cfg(feature = "neofs-grpc")]
use super::super::super::proto::neofs_v2;
#[cfg(feature = "neofs-grpc")]
use base64::Engine as _;

#[cfg(feature = "neofs-grpc")]
#[test]
fn neofs_json_header_matches_csharp_format() {
    let container_bytes = vec![1u8; 32];
    let owner_bytes = vec![2u8; 25];
    let checksum_bytes = vec![3u8; 32];
    let container_b64 =
        base64::engine::general_purpose::STANDARD.encode(&container_bytes);
    let owner_b64 = base64::engine::general_purpose::STANDARD.encode(&owner_bytes);
    let checksum_b64 =
        base64::engine::general_purpose::STANDARD.encode(&checksum_bytes);

    let header = neofs_v2::object::Header {
        version: Some(neofs_v2::refs::Version { major: 2, minor: 11 }),
        container_id: Some(neofs_v2::refs::ContainerId {
            value: container_bytes,
        }),
        owner_id: Some(neofs_v2::refs::OwnerId { value: owner_bytes }),
        creation_epoch: 42,
        payload_length: 512,
        payload_hash: Some(neofs_v2::refs::Checksum {
            r#type: neofs_v2::refs::ChecksumType::Sha256 as i32,
            sum: checksum_bytes,
        }),
        object_type: neofs_v2::object::ObjectType::Lock as i32,
        homomorphic_hash: None,
        session_token: None,
        attributes: vec![neofs_v2::object::header::Attribute {
            key: "k".to_string(),
            value: "v".to_string(),
        }],
        split: None,
        session_token_v2: None,
    };

    let json = neofs_json_header(&header).expect("header json");
    let expected = format!(
        "{{ \"version\": {{ \"major\": 2, \"minor\": 11 }}, \
\"containerID\": {{ \"value\": \"{container_b64}\" }}, \
\"ownerID\": {{ \"value\": \"{owner_b64}\" }}, \
\"creationEpoch\": \"42\", \
\"payloadLength\": \"512\", \
\"payloadHash\": {{ \"type\": \"SHA256\", \"sum\": \"{checksum_b64}\" }}, \
\"objectType\": \"LOCK\", \
\"attributes\": [ {{ \"key\": \"k\", \"value\": \"v\" }} ] }}"
    );
    assert_eq!(json, expected);
}
