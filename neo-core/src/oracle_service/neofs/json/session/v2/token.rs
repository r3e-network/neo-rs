use super::super::super::super::proto::neofs_v2;
use super::super::super::helpers::{json_string, json_u64_string, push_json_field};
use super::super::super::object::{neofs_json_owner_id, neofs_json_signature};
use super::context::neofs_json_session_context_v2;

pub(crate) fn neofs_json_session_token_v2(
    token: &neofs_v2::session::SessionTokenV2,
) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if let Some(body) = token
        .body
        .as_ref()
        .and_then(neofs_json_session_token_v2_body)
    {
        push_json_field(&mut out, &mut first, "body", &body);
    }
    if let Some(signature) = token.signature.as_ref().and_then(neofs_json_signature) {
        push_json_field(&mut out, &mut first, "signature", &signature);
    }
    if let Some(origin) = token.origin.as_ref() {
        let json =
            neofs_json_session_token_v2(origin.as_ref()).unwrap_or_else(|| "{ }".to_string());
        push_json_field(&mut out, &mut first, "origin", &json);
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}

fn neofs_json_session_token_v2_body(
    body: &neofs_v2::session::session_token_v2::Body,
) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if body.version != 0 {
        push_json_field(&mut out, &mut first, "version", &body.version.to_string());
    }
    if body.nonce != 0 {
        push_json_field(&mut out, &mut first, "nonce", &body.nonce.to_string());
    }
    if let Some(issuer) = body.issuer.as_ref().and_then(neofs_json_owner_id) {
        push_json_field(&mut out, &mut first, "issuer", &issuer);
    }
    if !body.subjects.is_empty() {
        let mut subjects_json = String::from("[ ");
        for (idx, subject) in body.subjects.iter().enumerate() {
            if idx > 0 {
                subjects_json.push_str(", ");
            }
            if let Some(subject_json) = neofs_json_session_target(subject) {
                subjects_json.push_str(&subject_json);
            } else {
                subjects_json.push_str("{ }");
            }
        }
        subjects_json.push_str(" ]");
        push_json_field(&mut out, &mut first, "subjects", &subjects_json);
    }
    if let Some(lifetime) = body.lifetime.as_ref().and_then(neofs_json_token_lifetime) {
        push_json_field(&mut out, &mut first, "lifetime", &lifetime);
    }
    if !body.contexts.is_empty() {
        let mut contexts_json = String::from("[ ");
        for (idx, context) in body.contexts.iter().enumerate() {
            if idx > 0 {
                contexts_json.push_str(", ");
            }
            if let Some(context_json) = neofs_json_session_context_v2(context) {
                contexts_json.push_str(&context_json);
            } else {
                contexts_json.push_str("{ }");
            }
        }
        contexts_json.push_str(" ]");
        push_json_field(&mut out, &mut first, "contexts", &contexts_json);
    }
    if body.r#final {
        push_json_field(&mut out, &mut first, "final", "true");
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}

fn neofs_json_session_target(target: &neofs_v2::session::Target) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if let Some(identifier) = target.identifier.as_ref() {
        match identifier {
            neofs_v2::session::target::Identifier::OwnerId(owner) => {
                if let Some(owner_json) = neofs_json_owner_id(owner) {
                    push_json_field(&mut out, &mut first, "ownerID", &owner_json);
                }
            }
            neofs_v2::session::target::Identifier::NnsName(name) => {
                push_json_field(&mut out, &mut first, "nnsName", &json_string(name));
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

fn neofs_json_token_lifetime(lifetime: &neofs_v2::session::TokenLifetime) -> Option<String> {
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
