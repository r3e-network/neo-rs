use super::*;
use neo_payloads::{Signer, Transaction};
use neo_primitives::WitnessScope;
use neo_vm::OpCode;
use serde_json::json;

/// Builds a signed transaction (with a witness) for context round-trip tests.
fn signed_transaction() -> Transaction {
    let witness = Witness::new_with_scripts(vec![OpCode::PUSH1.byte()], vec![OpCode::PUSH1.byte()]);
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_witnesses(vec![witness]);
    tx
}

#[test]
fn to_json_data_is_unsigned_and_hash_is_tx_hash() {
    let tx = signed_transaction();
    let expected_hash = tx.hash();
    // The unsigned serialization (no witnesses) is what C# writes into `data`.
    let unsigned = tx.hash_data();

    let snapshot = Arc::new(DataCache::new(false));
    let context = ContractParametersContext::new_with_type(
        snapshot,
        tx.clone(),
        860_833_102,
        Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
    );

    let json = context.to_json();

    // `hash` must equal the transaction hash (C# `Verifiable.Hash`).
    assert_eq!(
        json["hash"].as_str().unwrap(),
        expected_hash.to_string(),
        "context hash must be the tx hash"
    );

    // `data` must be the UNSIGNED form (no witnesses), Base64-encoded.
    let data_b64 = json["data"].as_str().unwrap();
    let decoded = general_purpose::STANDARD.decode(data_b64).unwrap();
    assert_eq!(
        decoded, unsigned,
        "context `data` must be the witness-less unsigned serialization"
    );

    // The full (with-witness) serialization is strictly longer than the unsigned form,
    // so `data` carrying the unsigned bytes proves the witnesses were dropped.
    let full = tx.to_bytes();
    assert!(
        decoded.len() < full.len(),
        "unsigned `data` ({} bytes) must be shorter than the signed tx ({} bytes)",
        decoded.len(),
        full.len()
    );

    assert_eq!(json["network"].as_u64().unwrap(), 860_833_102);
    assert_eq!(
        json["type"].as_str().unwrap(),
        "Neo.Network.P2P.Payloads.Transaction"
    );
}

#[test]
fn parse_transaction_context_reconstructs_unsigned_tx() {
    let tx = signed_transaction();
    let snapshot = Arc::new(DataCache::new(false));
    let context = ContractParametersContext::new_with_type(
        Arc::clone(&snapshot),
        tx.clone(),
        860_833_102,
        Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
    );

    let json_text = serde_json::to_string(&context.to_json()).unwrap();

    let (_parsed_context, parsed_tx) =
        ContractParametersContext::parse_transaction_context(&json_text, snapshot)
            .expect("witness-less context data must parse via deserialize_unsigned");

    // The reconstructed tx must match the original's identity (hash) and carry no witnesses.
    assert_eq!(
        parsed_tx.hash(),
        tx.hash(),
        "reconstructed tx hash must match the original"
    );
    assert!(
        parsed_tx.witnesses().is_empty(),
        "tx reconstructed from unsigned `data` must have no witnesses"
    );
    assert_eq!(parsed_tx.nonce(), tx.nonce());
    assert_eq!(parsed_tx.script(), tx.script());
}

#[test]
fn parse_transaction_context_rejects_hash_mismatch() {
    let tx = signed_transaction();
    let snapshot = Arc::new(DataCache::new(false));
    let context = ContractParametersContext::new_with_type(
        Arc::clone(&snapshot),
        tx.clone(),
        860_833_102,
        Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
    );

    let mut json = context.to_json();
    // Corrupt the hash so it no longer matches the reconstructed tx.
    json["hash"] = json!(UInt256::default().to_string());
    let json_text = serde_json::to_string(&json).unwrap();

    let result = ContractParametersContext::parse_transaction_context(&json_text, snapshot);
    let err = match result {
        Ok(_) => panic!("a hash that disagrees with `data` must be rejected (C# parity)"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("does not match"),
        "error should explain the hash mismatch: {err}"
    );
}

#[test]
fn cross_parse_csharp_shaped_transaction_context() {
    // A C#-shaped context JSON: `data` is Base64 of the UNSIGNED tx, `hash` is the tx hash,
    // `items` is empty (unsigned, awaiting signatures), `network`/`type` as C# emits.
    let tx = signed_transaction();
    let data_b64 = general_purpose::STANDARD.encode(tx.hash_data());
    let hash = tx.hash().to_string();

    let json = json!({
        "type": "Neo.Network.P2P.Payloads.Transaction",
        "hash": hash,
        "data": data_b64,
        "items": {},
        "network": 860_833_102u32,
    });
    let json_text = serde_json::to_string(&json).unwrap();

    let snapshot = Arc::new(DataCache::new(false));
    let (_context, parsed_tx) =
        ContractParametersContext::parse_transaction_context(&json_text, snapshot)
            .expect("C#-shaped context must parse");

    assert_eq!(parsed_tx.hash(), tx.hash());
    assert!(parsed_tx.witnesses().is_empty());
}

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
