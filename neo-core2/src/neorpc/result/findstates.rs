use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct FindStates {
    results: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_proof: Option<ProofWithKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_proof: Option<ProofWithKey>,
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
pub struct KeyValue {
    key: Vec<u8>,
    value: Vec<u8>,
}
