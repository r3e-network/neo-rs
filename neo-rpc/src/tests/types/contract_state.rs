use super::*;
use crate::types::test_fixtures::rpc_case_result;
use base64::{Engine as _, engine::general_purpose};
use neo_manifest::NefFile;
use neo_manifest::{ContractManifest, ContractMethodDescriptor};
use neo_primitives::ContractParameterType;
use neo_serialization::json::{JArray, JToken};

fn valid_manifest(name: &str) -> ContractManifest {
    let mut manifest = ContractManifest::new(name.to_string());
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "verify".to_string(),
            Vec::new(),
            ContractParameterType::Boolean,
            0,
            true,
        )
        .expect("valid method descriptor"),
    );
    manifest
}

#[test]
fn rpc_contract_state_parses_minimal_contract() {
    let mut json = JObject::new();
    json.insert("id".to_string(), JToken::Number(1f64));
    json.insert("updatecounter".to_string(), JToken::Number(2f64));
    json.insert(
        "hash".to_string(),
        JToken::String(UInt160::zero().to_string()),
    );

    let nef = NefFile {
        compiler: "neo".into(),
        source: "".into(),
        tokens: Vec::new(),
        script: vec![1, 2, 3],
        checksum: 123,
    };
    let mut nef_json = JObject::new();
    nef_json.insert("compiler".to_string(), JToken::String(nef.compiler.clone()));
    nef_json.insert("source".to_string(), JToken::String(nef.source.clone()));
    nef_json.insert("tokens".to_string(), JToken::Array(JArray::new()));
    nef_json.insert(
        "script".to_string(),
        JToken::String(general_purpose::STANDARD.encode(&nef.script)),
    );
    nef_json.insert("checksum".to_string(), JToken::Number(nef.checksum as f64));
    json.insert("nef".to_string(), JToken::Object(nef_json));

    let manifest = valid_manifest("TestContract");
    let manifest_value = manifest.to_json().expect("manifest json");
    let manifest_token =
        JToken::parse(&manifest_value.to_string(), 128).expect("neo-serialization::json parse");
    json.insert(
        "manifest".to_string(),
        JToken::Object(manifest_token.as_object().unwrap().clone()),
    );

    let parsed = RpcContractState::from_json(&json).expect("contract state");
    assert_eq!(parsed.contract_state.id, 1);
    assert_eq!(parsed.contract_state.update_counter, 2);
    assert_eq!(parsed.contract_state.hash, UInt160::zero());
    assert_eq!(parsed.contract_state.nef.checksum, 123);
    assert_eq!(parsed.contract_state.manifest.name, "TestContract");
}

#[test]
fn rpc_contract_state_roundtrip_json() {
    let nef = NefFile {
        compiler: "neo".into(),
        source: "src".into(),
        tokens: vec![neo_manifest::MethodToken::default()],
        script: vec![1, 2, 3],
        checksum: 321,
    };
    let manifest = valid_manifest("Contract");
    let state = RpcContractState {
        contract_state: ContractState {
            id: 5,
            update_counter: 6,
            hash: UInt160::zero(),
            nef,
            manifest,
        },
    };

    let json = state.to_json().expect("to_json");
    let parsed = RpcContractState::from_json(&json).expect("from_json");
    assert_eq!(parsed.contract_state.id, 5);
    assert_eq!(parsed.contract_state.update_counter, 6);
    assert_eq!(parsed.contract_state.nef.checksum, 321);
    assert_eq!(parsed.contract_state.manifest.name, "Contract");
}

#[test]
fn contract_state_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getcontractstateasync") else {
        return;
    };
    let parsed = RpcContractState::from_json(&expected).expect("parse");
    let actual = parsed.to_json().expect("to_json");
    assert_eq!(expected.to_string(), actual.to_string());
}
