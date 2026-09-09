//! Test harness for running protocol compliance tests

use super::ComplianceResult;
use super::test_vectors::block_validation::{BlockValidationVector, MainnetBlockVectors};
use neo_core::UInt256;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::Block;

/// Test harness for protocol compliance testing.
///
/// The vectors are real MainNet blocks captured from a live C# node (see
/// [`super::test_vectors::block_validation`]); [`ProtocolTestHarness::run_all`]
/// executes the same per-vector checks the `real_mainnet_blocks_*` tests
/// assert (hash reproduction, size, header fields, byte-for-byte round-trip).
pub struct ProtocolTestHarness {
    pub test_vectors: Vec<BlockValidationVector>,
}

impl ProtocolTestHarness {
    pub fn new() -> Self {
        Self {
            test_vectors: Vec::new(),
        }
    }

    /// Loads a vector file produced by `scripts/generate-block-test-vectors.py`.
    #[allow(dead_code)]
    pub fn load_vectors(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let doc: MainnetBlockVectors = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        self.test_vectors = doc.blocks;
        Ok(())
    }

    /// Runs every loaded vector through the compliance checks and reports one
    /// result per vector.
    ///
    /// Audit note (2026-09-08): this previously mapped every vector to
    /// `Inconclusive { reason: "Not implemented" }`; it now executes the real
    /// checks so the harness cannot silently pretend compliance.
    pub fn run_all(&self) -> Vec<ComplianceResult> {
        self.test_vectors.iter().map(run_vector).collect()
    }
}

/// Executes the C#-parity checks for one block vector.
fn run_vector(v: &BlockValidationVector) -> ComplianceResult {
    match run_vector_inner(v) {
        Ok(()) => ComplianceResult::Compliant,
        Err((reason, details)) => ComplianceResult::Divergent { reason, details },
    }
}

fn run_vector_inner(v: &BlockValidationVector) -> Result<(), (String, String)> {
    let bytes = from_hex(&v.block_hex);
    let mut reader = MemoryReader::new(&bytes);
    let mut block = Block::deserialize(&mut reader)
        .map_err(|e| ("block deserialization failed".to_string(), format!("{e:?}")))?;

    let expected_hash = UInt256::parse(&v.hash)
        .map_err(|e| ("malformed fixture hash".to_string(), format!("{e:?}")))?;
    if block.hash() != expected_hash {
        return Err((
            "block hash mismatch".to_string(),
            format!(
                "height {}: computed {}, expected {}",
                v.height,
                block.hash().to_hex_string(),
                v.hash
            ),
        ));
    }

    let size = Serializable::size(&block);
    if size != v.size {
        return Err((
            "serialized size mismatch".to_string(),
            format!("height {}: computed {size}, expected {}", v.height, v.size),
        ));
    }

    let expected_root = UInt256::parse(&v.merkleroot)
        .map_err(|e| ("malformed fixture merkleroot".to_string(), format!("{e:?}")))?;
    if *block.merkle_root() != expected_root {
        return Err((
            "merkle root mismatch".to_string(),
            format!(
                "height {}: computed {}",
                v.height,
                block.merkle_root().to_hex_string()
            ),
        ));
    }

    if block.index() != v.height {
        return Err((
            "index mismatch".to_string(),
            format!("computed {}, expected {}", block.index(), v.height),
        ));
    }
    if block.timestamp() != v.time {
        return Err((
            "timestamp mismatch".to_string(),
            format!(
                "height {}: computed {}, expected {}",
                v.height,
                block.timestamp(),
                v.time
            ),
        ));
    }
    if block.transactions.len() != v.tx_count {
        return Err((
            "transaction count mismatch".to_string(),
            format!(
                "height {}: computed {}, expected {}",
                v.height,
                block.transactions.len(),
                v.tx_count
            ),
        ));
    }

    let mut writer = BinaryWriter::new();
    block
        .serialize(&mut writer)
        .map_err(|e| ("re-serialization failed".to_string(), format!("{e:?}")))?;
    if to_hex(&writer.into_bytes()) != v.block_hex {
        return Err((
            "byte-for-byte round-trip mismatch".to_string(),
            format!("height {}", v.height),
        ));
    }

    Ok(())
}

fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{:02x}", b);
        s
    })
}
