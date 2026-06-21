use super::*;

#[test]
fn constructors_capture_message() {
    let err = NodeError::missing_service("consensus");
    assert_eq!(err.to_string(), "missing required service: consensus");
}

#[test]
fn result_alias_compiles() {
    let ok: NodeResult<u32> = Ok(1);
    if let Ok(v) = ok {
        assert_eq!(v, 1);
    } else {
        panic!("expected Ok");
    }
}
