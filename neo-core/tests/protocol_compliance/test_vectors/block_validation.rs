//! Test vectors for block validation from C# reference implementation
//!
//! Generate vectors with: scripts/generate-block-test-vectors.py
//! Compare implementations: scripts/compare-block-validation.py

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockValidationVector {
    pub height: u32,
    pub block_hex: String,
    pub hash: String,
    pub size: usize,
    pub merkleroot: String,
    pub time: u64,
    pub tx_count: usize,
}

/// Load test vectors from JSON file
pub fn load_vectors_from_json(json_str: &str) -> Vec<BlockValidationVector> {
    serde_json::from_str(json_str).expect("Failed to parse test vectors")
}

/// Test vectors from mainnet blocks (placeholder)
pub fn mainnet_block_vectors() -> Vec<BlockValidationVector> {
    // Run: scripts/generate-block-test-vectors.py --rpc <C#-node-url>
    vec![]
}
