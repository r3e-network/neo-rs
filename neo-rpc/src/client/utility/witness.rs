use base64::{Engine as _, engine::general_purpose};
use neo_error::{CoreError, CoreResult};
use neo_payloads::Witness;
use neo_payloads::witness::Witness as PayloadWitness;
// `invocation_script`/`verification_script` accessors come from this trait.
use neo_primitives::Witness as _;
use neo_serialization::json::JObject;

use super::parsing::parse_base64_token;

pub fn witness_from_json(json: &JObject) -> CoreResult<Witness> {
    let (invocation_bytes, verification_bytes) = parse_witness_scripts(json)?;
    Ok(Witness::new_with_scripts(
        invocation_bytes,
        verification_bytes,
    ))
}

pub fn payload_witness_from_json(json: &JObject) -> CoreResult<PayloadWitness> {
    let (invocation_bytes, verification_bytes) = parse_witness_scripts(json)?;
    Ok(PayloadWitness::new_with_scripts(
        invocation_bytes,
        verification_bytes,
    ))
}

pub fn scripts_to_witness_json(invocation: &[u8], verification: &[u8]) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "invocation".to_string(),
        neo_serialization::json::JToken::String(general_purpose::STANDARD.encode(invocation)),
    );
    json.insert(
        "verification".to_string(),
        neo_serialization::json::JToken::String(general_purpose::STANDARD.encode(verification)),
    );
    json
}

pub fn witness_to_json(witness: &Witness) -> JObject {
    scripts_to_witness_json(witness.invocation_script(), witness.verification_script())
}

pub fn payload_witness_to_json(witness: &PayloadWitness) -> JObject {
    scripts_to_witness_json(witness.invocation_script(), witness.verification_script())
}

fn parse_witness_scripts(json: &JObject) -> CoreResult<(Vec<u8>, Vec<u8>)> {
    let invocation = json
        .get("invocation")
        .ok_or_else(|| CoreError::other("Missing 'invocation' field"))?;
    let verification = json
        .get("verification")
        .ok_or_else(|| CoreError::other("Missing 'verification' field"))?;
    let invocation_bytes = parse_base64_token(invocation, "invocation")?;
    let verification_bytes = parse_base64_token(verification, "verification")?;
    Ok((invocation_bytes, verification_bytes))
}
