use super::*;
use neo_io::{BinaryWriter, MemoryReader};
use neo_vm::StackValue;

fn stack_value_struct_eq(a: &neo_vm::StackValue, b: &neo_vm::StackValue) -> bool {
    a.structural_eq(b)
}

fn stack_items_from_manifest(manifest: &ContractManifest) -> Vec<StackValue> {
    let StackValue::Struct(_, items) = manifest.to_stack_value() else {
        panic!("expected manifest Struct")
    };
    items
}

fn deployable_manifest_json() -> Value {
    serde_json::json!({
        "name": "sample",
        "groups": [],
        "features": {},
        "supportedstandards": [],
        "abi": {
            "methods": [{
                "name": "main",
                "parameters": [],
                "returntype": "Void",
                "offset": 0,
                "safe": false
            }],
            "events": []
        },
        "permissions": [],
        "trusts": "*",
        "extra": null
    })
}

/// Bug #10 regression — manifest `extra` JSON must use C# JavaScriptEncoder.Default
/// escape semantics. serde_json's minimal RFC-8259 escape set produces wrong bytes
/// for `&`, `<`, `>`, `'`, `+`, `` ` ``, and all non-ASCII. Block 1,208,916 deploy
/// of "Three Orange Hearts" NEP-11 had `&` in the description; serde_json kept it
/// literal, C# escaped it to `&`, state roots diverged from that block onward.
#[test]
fn extra_with_ampersand_uses_csharp_escape() {
    let m = ContractManifest {
        extra: Some(
            ManifestExtra::from_value(serde_json::json!({
                "description": "NEO, GAS, & FLM on Neo N3"
            }))
            .expect("object extra"),
        ),
        ..Default::default()
    };
    let value = m.to_stack_value();
    let StackValue::Struct(_, items) = value else {
        panic!("expected Struct")
    };
    let extra_item = &items[7];
    let StackValue::ByteString(extra_bytes) = extra_item else {
        panic!("expected extra ByteString")
    };
    let extra_str = std::str::from_utf8(extra_bytes).expect("utf-8");
    assert!(
        extra_str.contains("\\u0026"),
        "expected `&` to be escaped as `\\u0026`, got: {extra_str}"
    );
    assert!(
        !extra_str.contains('&'),
        "raw `&` must NOT appear in C#-compatible output, got: {extra_str}"
    );
}

#[test]
fn contract_manifest_projects_to_stack_value() {
    let mut manifest = ContractManifest::new("sample".to_string());
    manifest.supported_standards = vec!["NEP-17".to_string()];
    manifest.features.insert(
        "feature".to_string(),
        serde_json::json!({
            "description": "GAS & NEO"
        }),
    );
    manifest.extra = Some(
        ManifestExtra::from_value(serde_json::json!({
            "description": "NEO, GAS, & FLM on Neo N3"
        }))
        .expect("object extra"),
    );

    let value = manifest.to_stack_value();
    let StackValue::Struct(_, items) = value else {
        panic!("expected manifest Struct")
    };

    assert_eq!(items[0], StackValue::ByteString(b"sample".to_vec()));
    let expected_groups = StackValue::Array(neo_vm::next_stack_item_id(), Vec::new());
    assert!(
        stack_value_struct_eq(&items[1], &expected_groups),
        "structural StackValue mismatch: {:?} vs {expected_groups:?}",
        items[1]
    );
    let StackValue::Map(_, features) = &items[2] else {
        panic!("expected features map")
    };
    assert!(
        features.is_empty(),
        "C# ContractManifest.ToStackItem always emits an empty features map"
    );
    let expected_standards = StackValue::Array(
        neo_vm::next_stack_item_id(),
        vec![StackValue::ByteString(b"NEP-17".to_vec())],
    );
    assert!(
        stack_value_struct_eq(&items[3], &expected_standards),
        "structural StackValue mismatch: {:?} vs {expected_standards:?}",
        items[3]
    );
    let expected_abi = manifest.abi.to_stack_value();
    assert!(
        stack_value_struct_eq(&items[4], &expected_abi),
        "structural StackValue mismatch: {:?} vs {expected_abi:?}",
        items[4]
    );
    let expected_permissions = StackValue::Array(
        neo_vm::next_stack_item_id(),
        vec![manifest.permissions[0].to_stack_value()],
    );
    assert!(
        stack_value_struct_eq(&items[5], &expected_permissions),
        "structural StackValue mismatch: {:?} vs {expected_permissions:?}",
        items[5]
    );
    assert_eq!(items[6], StackValue::Null);
    let StackValue::ByteString(extra_bytes) = &items[7] else {
        panic!("expected extra ByteString")
    };
    let extra = std::str::from_utf8(extra_bytes).expect("extra utf8");
    assert!(
        extra.contains("\\u0026"),
        "extra should use C# JSON escapes"
    );
}

#[test]
fn contract_manifest_reads_stack_value() {
    let mut source = ContractManifest::new("sample".to_string());
    source.supported_standards = vec!["NEP-17".to_string()];
    source.extra = Some(
        ManifestExtra::from_value(serde_json::json!({"description": "ok"})).expect("object extra"),
    );

    let mut decoded = ContractManifest::default();
    decoded
        .from_stack_value(source.to_stack_value())
        .expect("manifest from stack value");

    assert_eq!(decoded.name, source.name);
    assert_eq!(decoded.supported_standards, source.supported_standards);
    assert_eq!(decoded.abi, source.abi);
    assert_eq!(decoded.permissions, source.permissions);
    assert_eq!(decoded.trusts, source.trusts);
    assert_eq!(decoded.extra, source.extra);
}

#[test]
fn contract_manifest_parse_uses_csharp_json_field_rules() {
    let manifest = ContractManifest::parse(&deployable_manifest_json().to_string())
        .expect("valid manifest parses");
    assert_eq!(manifest.name, "sample");

    let mut empty_methods_permission = deployable_manifest_json();
    empty_methods_permission["permissions"] =
        serde_json::json!([{ "contract": "*", "methods": [] }]);
    let manifest = ContractManifest::parse(&empty_methods_permission.to_string())
        .expect("C# permits empty permission method lists");
    manifest
        .validate()
        .expect("empty method list remains valid");

    let mut bad_parameter = deployable_manifest_json();
    bad_parameter["abi"]["methods"][0]["parameters"] =
        serde_json::json!([{ "name": "bad", "type": "Void" }]);
    assert!(ContractManifest::parse(&bad_parameter.to_string()).is_err());

    let mut missing_features = deployable_manifest_json();
    missing_features.as_object_mut().unwrap().remove("features");
    assert!(ContractManifest::parse(&missing_features.to_string()).is_err());

    let mut missing_trusts = deployable_manifest_json();
    missing_trusts.as_object_mut().unwrap().remove("trusts");
    assert!(ContractManifest::parse(&missing_trusts.to_string()).is_err());
}

#[test]
fn contract_manifest_parse_rejects_duplicate_json_entries() {
    let mut duplicate_standards = deployable_manifest_json();
    duplicate_standards["supportedstandards"] = serde_json::json!(["NEP-17", "NEP-17"]);
    assert!(ContractManifest::parse(&duplicate_standards.to_string()).is_err());

    let mut duplicate_permissions = deployable_manifest_json();
    duplicate_permissions["permissions"] = serde_json::json!([
        { "contract": "*", "methods": "*" },
        { "contract": "*", "methods": "*" }
    ]);
    assert!(ContractManifest::parse(&duplicate_permissions.to_string()).is_err());

    let mut duplicate_trusts = deployable_manifest_json();
    duplicate_trusts["trusts"] = serde_json::json!(["*", "*"]);
    assert!(ContractManifest::parse(&duplicate_trusts.to_string()).is_err());
}

#[test]
fn contract_manifest_validate_rejects_constructed_invariants() {
    let assert_invalid = |mutate: fn(&mut ContractManifest)| {
        let mut manifest = ContractManifest::new("sample".to_string());
        mutate(&mut manifest);
        assert!(manifest.validate().is_err());
    };

    assert_invalid(|manifest| {
        manifest.name.clear();
    });
    assert_invalid(|manifest| {
        manifest
            .features
            .insert("feature".to_string(), serde_json::json!({}));
    });
    assert_invalid(|manifest| {
        manifest.supported_standards = vec!["NEP-17".to_string(), "NEP-17".to_string()];
    });
    assert_invalid(|manifest| {
        manifest.permissions = vec![
            ContractPermission::default_wildcard(),
            ContractPermission::default_wildcard(),
        ];
    });
    assert_invalid(|manifest| {
        manifest.trusts = WildCardContainer::create(vec![
            ContractPermissionDescriptor::create_wildcard(),
            ContractPermissionDescriptor::create_wildcard(),
        ]);
    });
}

#[test]
fn manifest_extra_wrapper_accepts_only_json_objects() {
    assert!(ManifestExtra::from_value(serde_json::json!({"description": "ok"})).is_ok());
    assert!(ManifestExtra::from_value(serde_json::Value::Null).is_err());
    assert!(ManifestExtra::from_value(serde_json::json!(["not", "object"])).is_err());
    assert!(ManifestExtra::from_value(serde_json::json!("not object")).is_err());
}

#[test]
fn contract_manifest_binary_wire_round_trips_and_size_matches_bytes() {
    let manifest = ContractManifest::parse(&deployable_manifest_json().to_string())
        .expect("valid manifest parses");
    let mut writer = BinaryWriter::new();
    manifest.serialize(&mut writer).expect("serialize manifest");
    let bytes = writer.into_bytes();

    assert_eq!(manifest.size(), bytes.len());

    let mut reader = MemoryReader::new(&bytes);
    let decoded = ContractManifest::deserialize(&mut reader).expect("deserialize manifest");

    assert_eq!(decoded, manifest);
    assert_eq!(reader.position(), bytes.len());
}

#[test]
fn contract_manifest_rejects_non_empty_features_stack_value_like_csharp() {
    let source = ContractManifest::new("sample".to_string());
    let mut items = stack_items_from_manifest(&source);
    items[2] = StackValue::Map(
        neo_vm::next_stack_item_id(),
        vec![(
            StackValue::ByteString(b"feature".to_vec()),
            StackValue::ByteString(b"{}".to_vec()),
        )],
    );

    assert!(
        ContractManifest::default()
            .from_stack_value(StackValue::Struct(neo_vm::next_stack_item_id(), items))
            .is_err()
    );
}

#[test]
fn contract_manifest_rejects_malformed_stack_fields_like_csharp() {
    let assert_rejected = |mutate: fn(&mut Vec<StackValue>)| {
        let source = ContractManifest::new("sample".to_string());
        let mut items = stack_items_from_manifest(&source);
        mutate(&mut items);
        assert!(
            ContractManifest::default()
                .from_stack_value(StackValue::Struct(neo_vm::next_stack_item_id(), items))
                .is_err()
        );
    };

    assert_rejected(|items| {
        items[1] = StackValue::Array(neo_vm::next_stack_item_id(), vec![StackValue::Null]);
    });
    assert_rejected(|items| {
        items[3] = StackValue::Array(
            neo_vm::next_stack_item_id(),
            vec![StackValue::ByteString(vec![0xff])],
        );
    });
    assert_rejected(|items| {
        items[3] = StackValue::Array(neo_vm::next_stack_item_id(), vec![StackValue::Null]);
    });
    assert_rejected(|items| {
        items[6] = StackValue::Array(
            neo_vm::next_stack_item_id(),
            vec![StackValue::ByteString(vec![1, 2, 3])],
        );
    });
    assert_rejected(|items| {
        items[7] = StackValue::Null;
    });
    assert_rejected(|items| {
        items[7] = StackValue::ByteString(b"[]".to_vec());
    });
}
