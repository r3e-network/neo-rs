use super::{WitnessScope, WitnessScopes};

#[test]
fn witness_scopes_string_roundtrip() {
    let mut scopes = WitnessScopes::new();
    scopes.add_scope(WitnessScope::CalledByEntry);
    scopes.add_scope(WitnessScope::CustomContracts);
    let text = scopes.to_string();
    assert_eq!(text, "CalledByEntry|CustomContracts");
    let parsed = text.parse::<WitnessScopes>().unwrap();
    assert!(parsed.has_scope(WitnessScope::CalledByEntry));
    assert!(parsed.has_scope(WitnessScope::CustomContracts));
}

#[test]
fn witness_scopes_global_and_none() {
    let global = WitnessScopes::from_bits(WitnessScope::Global as u8);
    assert_eq!(global.to_string(), "Global");
    let parsed = "Global".parse::<WitnessScopes>().unwrap();
    assert!(parsed.has_scope(WitnessScope::Global));

    let none = WitnessScopes::new();
    assert_eq!(none.to_string(), "None");
    let parsed_none = "None".parse::<WitnessScopes>().unwrap();
    assert!(!parsed_none.has_scope(WitnessScope::CalledByEntry));
}
