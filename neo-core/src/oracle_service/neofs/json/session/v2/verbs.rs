use super::super::super::helpers::json_string;

pub(super) fn neofs_session_verb_name(value: i32) -> String {
    match value {
        0 => json_string("VERB_UNSPECIFIED"),
        1 => json_string("OBJECT_PUT"),
        2 => json_string("OBJECT_GET"),
        3 => json_string("OBJECT_HEAD"),
        4 => json_string("OBJECT_SEARCH"),
        5 => json_string("OBJECT_DELETE"),
        6 => json_string("OBJECT_RANGE"),
        7 => json_string("OBJECT_RANGEHASH"),
        8 => json_string("CONTAINER_PUT"),
        9 => json_string("CONTAINER_DELETE"),
        10 => json_string("CONTAINER_SETEACL"),
        11 => json_string("CONTAINER_SETATTRIBUTE"),
        12 => json_string("CONTAINER_REMOVEATTRIBUTE"),
        _ => value.to_string(),
    }
}
