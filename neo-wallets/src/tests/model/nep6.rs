//! Tests for NEP-6 (de)serialization parity with C# `Neo.Wallets.NEP6`.

use super::*;

/// A short, recognizable verification-script-like byte string.
/// `0c21` = PUSHDATA1 33, then 5 arbitrary bytes (not a real key, just a fixture).
const SCRIPT_BYTES: [u8; 7] = [0x0c, 0x21, 0x03, 0x4a, 0x1b, 0x2c, 0x3d];

fn sample_contract_file_base64() -> Nep6ContractFile {
    Nep6ContractFile {
        // Base64 of SCRIPT_BYTES, exactly as C# `Convert.ToBase64String` would emit.
        script: general_purpose::STANDARD.encode(SCRIPT_BYTES),
        parameters: vec![Nep6ContractParameterFile {
            name: "signature".to_string(),
            param_type: "Signature".to_string(),
        }],
        deployed: false,
    }
}

#[test]
fn nep6_contract_script_is_base64_not_hex_on_serialize() {
    // Build a Contract whose script is SCRIPT_BYTES and serialize it back to file form.
    let contract = Contract::create(
        vec![ContractParameterType::Signature],
        SCRIPT_BYTES.to_vec(),
    );
    let file = Nep6Contract::to_file(&contract, &["signature".to_string()]);

    // C# NEP6Contract.ToJson emits Base64: `Convert.ToBase64String(Script)`.
    let expected_b64 = general_purpose::STANDARD.encode(SCRIPT_BYTES);
    assert_eq!(
        file.script, expected_b64,
        "NEP-6 contract.script must be Base64-encoded to interoperate with neo-cli/neo-gui"
    );

    // Guard against a regression back to hex.
    let hex = neo_primitives::hex_util::encode_hex(&SCRIPT_BYTES);
    assert_ne!(
        file.script, hex,
        "NEP-6 contract.script must NOT be hex-encoded (C# uses Base64)"
    );
}

#[test]
fn nep6_contract_round_trips_through_base64() {
    let original = Contract::create(
        vec![ContractParameterType::Signature],
        SCRIPT_BYTES.to_vec(),
    );

    let file = Nep6Contract::to_file(&original, &["signature".to_string()]);
    let parsed = Nep6Contract::from_file(&file).expect("round-trip parse must succeed");

    assert_eq!(
        parsed.contract.script,
        SCRIPT_BYTES.to_vec(),
        "script bytes must survive the Base64 round-trip"
    );
    assert_eq!(parsed.parameter_names, vec!["signature".to_string()]);
    assert_eq!(
        parsed.contract.parameter_list,
        vec![ContractParameterType::Signature]
    );
}

#[test]
fn nep6_contract_parses_csharp_shaped_base64_fixture() {
    // A minimal, hand-written NEP6Contract JSON exactly as C# neo-cli would write it:
    // `script` is Base64, `parameters` is [{name,type}], `deployed` is a bool.
    let json = serde_json::json!({
        "script": general_purpose::STANDARD.encode(SCRIPT_BYTES),
        "parameters": [ { "name": "signature", "type": "Signature" } ],
        "deployed": false
    });

    let file: Nep6ContractFile =
        serde_json::from_value(json).expect("C#-shaped contract JSON must deserialize");
    let parsed = Nep6Contract::from_file(&file).expect("C#-shaped contract must parse");

    assert_eq!(
        parsed.contract.script,
        SCRIPT_BYTES.to_vec(),
        "Base64 `script` from a C# wallet must decode to the original bytes"
    );
}

#[test]
fn nep6_contract_rejects_hex_script_as_base64() {
    // A hex-encoded script (the OLD, buggy neo-rs format) is not valid Base64 for these
    // bytes and must not silently decode to the wrong value. `0c21034a1b2c3d` contains no
    // characters outside the Base64 alphabet, but its length (14) is not a multiple of 4,
    // so strict Base64 decoding rejects it — proving hex and Base64 are not interchangeable.
    let hex = neo_primitives::hex_util::encode_hex(&SCRIPT_BYTES);
    let file = Nep6ContractFile {
        script: hex,
        parameters: sample_contract_file_base64().parameters,
        deployed: false,
    };
    assert!(
        Nep6Contract::from_file(&file).is_err(),
        "a hex-encoded script must not be accepted as Base64"
    );
}
