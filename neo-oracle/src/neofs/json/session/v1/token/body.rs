use super::super::super::super::super::proto::neofs_v2;
use super::super::super::super::helpers::{json_string, json_u64_string, push_json_field};
use super::super::super::super::object::neofs_json_owner_id;
use super::super::context::{
    neofs_json_container_session_context, neofs_json_object_session_context,
};
use base64::Engine as _;

pub(super) fn neofs_json_session_token_body(
    body: &neofs_v2::session::session_token::Body,
) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if !body.id.is_empty() {
        let id_b64 = base64::engine::general_purpose::STANDARD.encode(&body.id);
        push_json_field(&mut out, &mut first, "id", &json_string(&id_b64));
    }
    if let Some(owner) = body.owner_id.as_ref().and_then(neofs_json_owner_id) {
        push_json_field(&mut out, &mut first, "ownerID", &owner);
    }
    if let Some(lifetime) = body
        .lifetime
        .as_ref()
        .and_then(neofs_json_session_token_lifetime)
    {
        push_json_field(&mut out, &mut first, "lifetime", &lifetime);
    }
    if !body.session_key.is_empty() {
        let key_b64 = base64::engine::general_purpose::STANDARD.encode(&body.session_key);
        push_json_field(&mut out, &mut first, "sessionKey", &json_string(&key_b64));
    }
    if let Some(context) = body.context.as_ref() {
        match context {
            neofs_v2::session::session_token::body::Context::Object(object) => {
                let json =
                    neofs_json_object_session_context(object).unwrap_or_else(|| "{ }".to_string());
                push_json_field(&mut out, &mut first, "object", &json);
            }
            neofs_v2::session::session_token::body::Context::Container(container) => {
                let json = neofs_json_container_session_context(container)
                    .unwrap_or_else(|| "{ }".to_string());
                push_json_field(&mut out, &mut first, "container", &json);
            }
        }
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}

fn neofs_json_session_token_lifetime(
    lifetime: &neofs_v2::session::session_token::body::TokenLifetime,
) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if lifetime.exp != 0 {
        push_json_field(&mut out, &mut first, "exp", &json_u64_string(lifetime.exp));
    }
    if lifetime.nbf != 0 {
        push_json_field(&mut out, &mut first, "nbf", &json_u64_string(lifetime.nbf));
    }
    if lifetime.iat != 0 {
        push_json_field(&mut out, &mut first, "iat", &json_u64_string(lifetime.iat));
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}
