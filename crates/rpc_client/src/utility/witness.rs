use base64::{engine::general_purpose, Engine as _};
use neo_core::network::p2p::payloads::witness::Witness as PayloadWitness;
use neo_core::Witness;
use neo_json::JObject;

use crate::utility::parsing::parse_base64_token;

pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
    let (invocation_bytes, verification_bytes) = parse_witness_scripts(json)?;

    Ok(Witness::new_with_scripts(
        invocation_bytes,
        verification_bytes,
    ))
}

pub fn payload_witness_from_json(json: &JObject) -> Result<PayloadWitness, String> {
    let (invocation_bytes, verification_bytes) = parse_witness_scripts(json)?;
    Ok(PayloadWitness::new_with_scripts(
        invocation_bytes,
        verification_bytes,
    ))
}

pub fn scripts_to_witness_json(invocation: &[u8], verification: &[u8]) -> neo_json::JObject {
    let mut json = neo_json::JObject::new();
    json.insert(
        "invocation".to_string(),
        neo_json::JToken::String(general_purpose::STANDARD.encode(invocation)),
    );
    json.insert(
        "verification".to_string(),
        neo_json::JToken::String(general_purpose::STANDARD.encode(verification)),
    );
    json
}

pub fn witness_to_json(witness: &neo_core::Witness) -> neo_json::JObject {
    scripts_to_witness_json(witness.invocation_script(), witness.verification_script())
}

pub fn payload_witness_to_json(witness: &PayloadWitness) -> neo_json::JObject {
    scripts_to_witness_json(witness.invocation_script(), witness.verification_script())
}

fn parse_witness_scripts(json: &JObject) -> Result<(Vec<u8>, Vec<u8>), String> {
    let invocation = json.get("invocation").ok_or("Missing 'invocation' field")?;
    let verification = json
        .get("verification")
        .ok_or("Missing 'verification' field")?;

    let invocation_bytes = parse_base64_token(invocation, "invocation")?;
    let verification_bytes = parse_base64_token(verification, "verification")?;

    Ok((invocation_bytes, verification_bytes))
}
