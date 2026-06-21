use super::*;
use neo_serialization::json::JToken;

#[test]
fn parses_method_token() {
    let mut json = JObject::new();
    json.insert(
        "hash".to_string(),
        JToken::String("0000000000000000000000000000000000000000".to_string()),
    );
    json.insert(
        "method".to_string(),
        JToken::String("balanceOf".to_string()),
    );
    json.insert("paramcount".to_string(), JToken::Number(1f64));
    json.insert("hasreturnvalue".to_string(), JToken::Boolean(true));
    json.insert(
        "callflags".to_string(),
        JToken::String("ReadOnly".to_string()),
    );

    let parsed = RpcMethodToken::from_json(&json).unwrap();
    assert_eq!(parsed.method_token.method, "balanceOf");
    assert!(parsed.method_token.has_return_value);
    assert_eq!(parsed.method_token.parameters_count, 1);
    assert!(
        parsed
            .method_token
            .call_flags
            .contains(CallFlags::READ_ONLY)
    );
}

#[test]
fn parses_numeric_flags_and_paramcount_strings() {
    let mut json = JObject::new();
    json.insert(
        "hash".to_string(),
        JToken::String("0000000000000000000000000000000000000000".to_string()),
    );
    json.insert("method".to_string(), JToken::String("transfer".to_string()));
    json.insert("paramcount".to_string(), JToken::String("2".to_string()));
    json.insert("hasreturnvalue".to_string(), JToken::Boolean(true));
    json.insert("callflags".to_string(), JToken::Number(3f64));

    let parsed = RpcMethodToken::from_json(&json).unwrap();
    assert_eq!(parsed.method_token.parameters_count, 2);
    assert!(
        parsed
            .method_token
            .call_flags
            .contains(CallFlags::READ_STATES)
    );
    assert!(
        parsed
            .method_token
            .call_flags
            .contains(CallFlags::WRITE_STATES)
    );
}

#[test]
fn method_token_roundtrip_json() {
    let token = RpcMethodToken {
        method_token: MethodToken {
            hash: UInt160::zero(),
            method: "transfer".into(),
            parameters_count: 2,
            has_return_value: true,
            call_flags: CallFlags::ALL,
        },
    };
    let json = token.to_json();
    let parsed = RpcMethodToken::from_json(&json).expect("method token");
    assert_eq!(parsed.method_token.method, token.method_token.method);
    assert_eq!(parsed.method_token.call_flags, CallFlags::ALL);
}

#[test]
fn method_token_to_json_uses_named_flags() {
    let token = RpcMethodToken {
        method_token: MethodToken {
            hash: UInt160::from([
                0x0e, 0x1b, 0x9b, 0xfa, 0xa4, 0x4e, 0x60, 0x31, 0x1f, 0x6f, 0x3c, 0x96, 0xcf,
                0xcd, 0x6d, 0x12, 0xc2, 0xfc, 0x3a, 0xdd,
            ]),
            method: "test".into(),
            parameters_count: 1,
            has_return_value: true,
            call_flags: CallFlags::ALL,
        },
    };

    let json = token.to_json();
    assert_eq!(
        json.get("callflags")
            .and_then(|value| value.as_string())
            .unwrap_or_default(),
        "All"
    );
}
