use super::super::super::super::proto::neofs_v2;
use super::super::super::helpers::push_json_field;
use super::super::super::object::neofs_json_container_id;
use super::verbs::neofs_session_verb_name;

pub(super) fn neofs_json_session_context_v2(
    context: &neofs_v2::session::SessionContextV2,
) -> Option<String> {
    let mut out = String::from("{ ");
    let mut first = true;

    if let Some(container) = context.container.as_ref().and_then(neofs_json_container_id) {
        push_json_field(&mut out, &mut first, "container", &container);
    }
    if !context.verbs.is_empty() {
        let mut verbs_json = String::from("[ ");
        for (idx, verb) in context.verbs.iter().enumerate() {
            if idx > 0 {
                verbs_json.push_str(", ");
            }
            verbs_json.push_str(&neofs_session_verb_name(*verb));
        }
        verbs_json.push_str(" ]");
        push_json_field(&mut out, &mut first, "verbs", &verbs_json);
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    Some(out)
}
