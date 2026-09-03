use super::super::super::helpers::json_string;

pub(super) fn neofs_object_session_verb_name(value: i32) -> String {
    match value {
        0 => json_string("VERB_UNSPECIFIED"),
        1 => json_string("PUT"),
        2 => json_string("GET"),
        3 => json_string("HEAD"),
        4 => json_string("SEARCH"),
        5 => json_string("DELETE"),
        6 => json_string("RANGE"),
        7 => json_string("RANGEHASH"),
        _ => value.to_string(),
    }
}

pub(super) fn neofs_container_session_verb_name(value: i32) -> String {
    match value {
        0 => json_string("VERB_UNSPECIFIED"),
        1 => json_string("PUT"),
        2 => json_string("DELETE"),
        3 => json_string("SETEACL"),
        4 => json_string("SETATTRIBUTE"),
        5 => json_string("REMOVEATTRIBUTE"),
        _ => value.to_string(),
    }
}
