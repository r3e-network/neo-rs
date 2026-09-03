use super::super::helpers::{
    base64_from_base58, header_str, json_string, json_u64_string, push_json_field,
};
use super::attributes::{build_neofs_attribute_json, build_neofs_attribute_list};

pub(crate) fn build_neofs_header_payload(headers: &reqwest::header::HeaderMap) -> String {
    let mut payload = String::from("{ ");
    let mut first = true;

    if let Some(value) = header_str(headers, "X-Object-Id") {
        if let Some(b64) = base64_from_base58(&value, Some(32)) {
            let object_id = format!("{{ \"value\": {} }}", json_string(&b64));
            push_json_field(&mut payload, &mut first, "objectID", &object_id);
        }
    }

    if let Some(header_payload) = build_neofs_header_proto(headers) {
        push_json_field(&mut payload, &mut first, "header", &header_payload);
    }

    if first {
        payload.push('}');
    } else {
        payload.push_str(" }");
    }
    payload
}

fn build_neofs_header_proto(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let mut header = String::from("{ ");
    let mut first = true;

    if let Some(value) = header_str(headers, "X-Container-Id") {
        if let Some(b64) = base64_from_base58(&value, Some(32)) {
            let container_id = format!("{{ \"value\": {} }}", json_string(&b64));
            push_json_field(&mut header, &mut first, "containerID", &container_id);
        }
    }

    if let Some(value) = header_str(headers, "X-Owner-Id") {
        if let Some(b64) = base64_from_base58(&value, Some(25)) {
            let owner_id = format!("{{ \"value\": {} }}", json_string(&b64));
            push_json_field(&mut header, &mut first, "ownerID", &owner_id);
        }
    }

    if let Some(length) =
        header_str(headers, "Content-Length").and_then(|value| value.parse::<u64>().ok())
    {
        push_json_field(
            &mut header,
            &mut first,
            "payloadLength",
            &json_u64_string(length),
        );
    }

    if let Some(attributes) = build_neofs_attribute_list(headers) {
        let attrs_json = build_neofs_attribute_json(&attributes);
        push_json_field(&mut header, &mut first, "attributes", &attrs_json);
    }

    if first {
        header.push('}');
    } else {
        header.push_str(" }");
    }
    Some(header)
}
