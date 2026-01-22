use super::super::super::proto::neofs_v2;
use std::convert::TryFrom;

pub fn neofs_json_header(header: &neofs_v2::object::Header) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if let Some(version) = header.version.as_ref() {
        push_json_field(
            &mut out,
            &mut first,
            "version",
            &format!(
                "{{ \"major\": {}, \"minor\": {} }}",
                version.major, version.minor
            ),
        );
    }

    if let Some(container) = header.container_id.as_ref() {
        if let Some(json) = neofs_json_container_id(container) {
            push_json_field(&mut out, &mut first, "containerID", &json);
        }
    }

    if let Some(owner) = header.owner_id.as_ref() {
        if let Some(json) = neofs_json_owner_id(owner) {
            push_json_field(&mut out, &mut first, "ownerID", &json);
        }
    }

    if header.creation_epoch != 0 {
        push_json_field(
            &mut out,
            &mut first,
            "creationEpoch",
            &header.creation_epoch.to_string(),
        );
    }

    if header.payload_length != 0 {
        push_json_field(
            &mut out,
            &mut first,
            "payloadLength",
            &header.payload_length.to_string(),
        );
    }

    if let Some(hash) = header.payload_hash.as_ref() {
        if let Some(json) = neofs_json_checksum(hash) {
            push_json_field(&mut out, &mut first, "payloadHash", &json);
        }
    }

    let object_type = match neofs_v2::object::ObjectType::try_from(header.object_type) {
        Ok(neofs_v2::object::ObjectType::Regular) => "REGULAR",
        Ok(neofs_v2::object::ObjectType::Lock) => "LOCK",
        Ok(neofs_v2::object::ObjectType::Tombstone) => "TOMBSTONE",
        _ => "UNKNOWN",
    };
    push_json_field(&mut out, &mut first, "objectType", object_type);

    if !header.attributes.is_empty() {
        let mut attrs_json = String::from("[ ");
        for (idx, attr) in header.attributes.iter().enumerate() {
            if idx > 0 {
                attrs_json.push_str(", ");
            }
            attrs_json.push_str(&format!(
                "{{ \"key\": \"{}\", \"value\": \"{}\" }}",
                attr.key, attr.value
            ));
        }
        attrs_json.push_str(" ]");
        push_json_field(&mut out, &mut first, "attributes", &attrs_json);
    }

    finish_json(&mut out, first);
    Some(out)
}

fn neofs_json_container_id(id: &neofs_v2::refs::ContainerId) -> Option<String> {
    use base64::Engine as _;
    let value = base64::engine::general_purpose::STANDARD.encode(&id.value);
    Some(format!("{{ \"value\": \"{value}\" }}"))
}

pub fn neofs_json_object_id(id: &neofs_v2::refs::ObjectId) -> Option<String> {
    use base64::Engine as _;
    let value = base64::engine::general_purpose::STANDARD.encode(&id.value);
    Some(format!("{{ \"value\": \"{value}\" }}"))
}

pub fn neofs_json_owner_id(id: &neofs_v2::refs::OwnerId) -> Option<String> {
    use base64::Engine as _;
    let value = base64::engine::general_purpose::STANDARD.encode(&id.value);
    Some(format!("{{ \"value\": \"{value}\" }}"))
}

fn neofs_json_checksum(checksum: &neofs_v2::refs::Checksum) -> Option<String> {
    use base64::Engine as _;
    let sum = base64::engine::general_purpose::STANDARD.encode(&checksum.sum);
    let hash_type = match neofs_v2::refs::ChecksumType::try_from(checksum.r#type) {
        Ok(neofs_v2::refs::ChecksumType::Sha256) => "SHA256",
        _ => "UNKNOWN",
    };
    Some(format!(
        "{{ \"type\": \"{hash_type}\", \"sum\": \"{sum}\" }}"
    ))
}

fn push_json_field(out: &mut String, first: &mut bool, key: &str, value: &str) {
    if !*first {
        out.push_str(", ");
    }
    *first = false;
    out.push_str(&format!("\"{key}\": {value}"));
}

fn finish_json(out: &mut String, first: bool) {
    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
}
