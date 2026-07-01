use super::*;
use serde_json::json;

#[test]
fn context_item_json_rejects_invalid_parameter_entry() {
    let json = json!({
        "script": null,
        "parameters": [
            { "type": "String", "value": "ok" },
            { "type": "NotAParameterType", "value": "dropped today" }
        ],
        "signatures": {}
    });

    let err = ContextItem::from_json(&json).expect_err("invalid parameter must fail decode");

    assert!(
        err.to_string().contains("parameters[1]"),
        "error should identify the bad parameter index: {err}"
    );
}

#[test]
fn context_item_json_rejects_invalid_signature_entry() {
    let json = json!({
        "script": null,
        "parameters": [],
        "signatures": {
            "not-hex": "not-base64"
        }
    });

    let err = ContextItem::from_json(&json).expect_err("invalid signature must fail decode");

    assert!(
        err.to_string().contains("signatures[not-hex]"),
        "error should identify the bad signature key: {err}"
    );
}
