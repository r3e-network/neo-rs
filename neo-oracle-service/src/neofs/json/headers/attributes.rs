use super::super::helpers::{header_str, json_string, push_json_field};

pub(super) fn build_neofs_attribute_list(
    headers: &reqwest::header::HeaderMap,
) -> Option<Vec<(String, String)>> {
    if let Some(value) = header_str(headers, "X-Attributes") {
        if let Ok(attrs) = serde_json::from_str::<std::collections::HashMap<String, String>>(&value)
        {
            if attrs.is_empty() {
                return None;
            }
            let mut list = attrs.into_iter().collect::<Vec<_>>();
            list.sort_by(|a, b| a.0.cmp(&b.0));
            return Some(list);
        }
    }

    build_neofs_attribute_headers(headers)
}

fn build_neofs_attribute_headers(
    headers: &reqwest::header::HeaderMap,
) -> Option<Vec<(String, String)>> {
    const PREFIX: &str = "X-Attribute-";
    let mut attrs = Vec::new();
    for (name, value) in headers.iter() {
        if let Some(key) = name.as_str().strip_prefix(PREFIX) {
            if let Ok(value) = value.to_str() {
                attrs.push((key.to_string(), value.to_string()));
            }
        }
    }
    if attrs.is_empty() {
        None
    } else {
        attrs.sort_by(|a, b| a.0.cmp(&b.0));
        Some(attrs)
    }
}

pub(super) fn build_neofs_attribute_json(attributes: &[(String, String)]) -> String {
    let mut json = String::from("[ ");
    for (idx, (key, value)) in attributes.iter().enumerate() {
        if idx > 0 {
            json.push_str(", ");
        }
        json.push_str("{ ");
        let mut first = true;
        push_json_field(&mut json, &mut first, "key", &json_string(key));
        push_json_field(&mut json, &mut first, "value", &json_string(value));
        if first {
            json.push('}');
        } else {
            json.push_str(" }");
        }
    }
    json.push_str(" ]");
    json
}
