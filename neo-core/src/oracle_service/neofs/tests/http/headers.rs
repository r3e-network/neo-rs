use super::super::super::json::build_neofs_header_payload;
use base64::Engine as _;
use reqwest::header::{HeaderMap, HeaderValue};

#[test]
fn build_neofs_header_payload_includes_fields() {
    let container_bytes = [1u8; 32];
    let object_bytes = [2u8; 32];
    let owner_bytes = [3u8; 25];
    let container = bs58::encode(container_bytes).into_string();
    let object = bs58::encode(object_bytes).into_string();
    let owner = bs58::encode(owner_bytes).into_string();
    let container_b64 = base64::engine::general_purpose::STANDARD.encode(container_bytes);
    let object_b64 = base64::engine::general_purpose::STANDARD.encode(object_bytes);
    let owner_b64 = base64::engine::general_purpose::STANDARD.encode(owner_bytes);

    let mut headers = HeaderMap::new();
    headers.insert("X-Container-Id", HeaderValue::from_str(&container).unwrap());
    headers.insert("X-Object-Id", HeaderValue::from_str(&object).unwrap());
    headers.insert("X-Owner-Id", HeaderValue::from_str(&owner).unwrap());
    headers.insert("Content-Length", HeaderValue::from_static("42"));
    headers.insert("X-Attributes", HeaderValue::from_static("{\"k\":\"v\"}"));

    let payload = build_neofs_header_payload(&headers);
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(parsed["objectID"]["value"], object_b64);
    assert_eq!(parsed["header"]["containerID"]["value"], container_b64);
    assert_eq!(parsed["header"]["ownerID"]["value"], owner_b64);
    assert_eq!(parsed["header"]["payloadLength"].as_str(), Some("42"));
    let attributes = parsed["header"]["attributes"]
        .as_array()
        .expect("attributes array");
    let mut attrs = std::collections::HashMap::new();
    for attr in attributes {
        let key = attr["key"].as_str().expect("attr key");
        let value = attr["value"].as_str().expect("attr value");
        attrs.insert(key.to_string(), value.to_string());
    }
    assert_eq!(attrs.get("k").map(String::as_str), Some("v"));
}

#[test]
fn build_neofs_header_payload_falls_back_to_attribute_headers() {
    let container_bytes = [1u8; 32];
    let container = bs58::encode(container_bytes).into_string();
    let container_b64 = base64::engine::general_purpose::STANDARD.encode(container_bytes);
    let mut headers = HeaderMap::new();
    headers.insert("X-Container-Id", HeaderValue::from_str(&container).unwrap());
    headers.insert("X-Attribute-Filename", HeaderValue::from_static("file.txt"));
    headers.insert("X-Attribute-Custom", HeaderValue::from_static("value"));

    let payload = build_neofs_header_payload(&headers);
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
    assert_eq!(parsed["header"]["containerID"]["value"], container_b64);
    let attributes = parsed["header"]["attributes"]
        .as_array()
        .expect("attributes array");
    let mut attrs = std::collections::HashMap::new();
    for attr in attributes {
        let key = attr["key"].as_str().expect("attr key");
        let value = attr["value"].as_str().expect("attr value");
        attrs.insert(key.to_string(), value.to_string());
    }
    assert_eq!(attrs.get("filename").map(String::as_str), Some("file.txt"));
    assert_eq!(attrs.get("custom").map(String::as_str), Some("value"));
}
