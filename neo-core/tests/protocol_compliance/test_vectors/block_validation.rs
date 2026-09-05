//! Test vectors for block validation from the C# reference implementation.
//!
//! The vectors are **real MainNet blocks** captured from a live Neo N3 node via
//! `scripts/generate-block-test-vectors.py`. Each entry carries the exact bytes
//! the C# node produced plus every field a compatible implementation must
//! reproduce (hash, size, merkle root, timestamp, transaction count).
//!
//! Regenerate with:
//!
//! ```text
//! python scripts/generate-block-test-vectors.py
//! ```

use serde::{Deserialize, Serialize};

/// A single MainNet block captured from a live node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockValidationVector {
    pub height: u32,
    /// Raw serialised block bytes, hex encoded.
    pub block_hex: String,
    /// `0x`-prefixed block hash reported by the C# node.
    pub hash: String,
    /// Serialised size in bytes reported by the C# node.
    pub size: usize,
    /// `0x`-prefixed merkle root of the transaction list.
    pub merkleroot: String,
    /// Block timestamp in milliseconds.
    pub time: u64,
    /// Number of transactions in the block.
    pub tx_count: usize,
    #[serde(default)]
    pub previousblockhash: String,
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub primary: u8,
    #[serde(default)]
    pub nextconsensus: String,
    /// Human-readable reason this height was captured (e.g. "Gorgon activation").
    #[serde(default)]
    pub note: String,
}

/// The checked-in vector file produced by the generator script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainnetBlockVectors {
    pub network: String,
    pub magic: u32,
    pub source: String,
    pub generated_at: String,
    pub chain_tip: u32,
    pub block_count: usize,
    pub blocks: Vec<BlockValidationVector>,
}

const MAINNET_BLOCKS_JSON: &str = include_str!("mainnet_blocks.json");

/// Loads the checked-in MainNet block vectors.
///
/// Panics on malformed data: a broken fixture must fail loudly rather than
/// silently skip the protocol checks that depend on it.
pub fn load_mainnet_vectors() -> MainnetBlockVectors {
    serde_json::from_str(MAINNET_BLOCKS_JSON).expect("mainnet_blocks.json must be valid")
}

/// The individual block vectors, sorted by height.
pub fn mainnet_block_vectors() -> Vec<BlockValidationVector> {
    load_mainnet_vectors().blocks
}

/// Load test vectors from a JSON string.
pub fn load_vectors_from_json(json_str: &str) -> Vec<BlockValidationVector> {
    serde_json::from_str(json_str).expect("Failed to parse test vectors")
}
