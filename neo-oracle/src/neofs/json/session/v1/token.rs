mod body;

use super::super::super::super::proto::neofs_v2;
use super::super::super::helpers::push_json_field;
use super::super::super::object::neofs_json_signature;
use body::neofs_json_session_token_body;

pub(crate) fn neofs_json_session_token(token: &neofs_v2::session::SessionToken) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if let Some(body) = token.body.as_ref().and_then(neofs_json_session_token_body) {
        push_json_field(&mut out, &mut first, "body", &body);
    }
    if let Some(signature) = token.signature.as_ref().and_then(neofs_json_signature) {
        push_json_field(&mut out, &mut first, "signature", &signature);
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}
