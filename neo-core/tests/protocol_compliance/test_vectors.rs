//! Test vectors for protocol compliance testing

use serde::{Deserialize, Serialize};

/// Test vector containing input and expected output from C# implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestVector {
    pub name: String,
    pub input: serde_json::Value,
    pub expected_output: serde_json::Value,
}

/// Load test vectors from JSON file
#[allow(dead_code)]
pub fn load_vectors(path: &str) -> Result<Vec<TestVector>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let vectors = serde_json::from_str(&content)?;
    Ok(vectors)
}
