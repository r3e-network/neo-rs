//! Block validation protocol compliance tests
//!
//! These tests require real test vectors generated from a C# Neo node.
//! They are marked `#[ignore]` because the vector files are currently empty/placeholder.
//! To populate them, run the C# vector generation tool and update `block_vectors.json`
//! and the genesis block hex.

#[cfg(test)]
mod tests {

    #[test]
    #[ignore = "test vectors not populated - block_vectors.json contains '[]'"]
    fn test_block_validation_vectors() {
        // Load test vectors generated from C# node
        let vectors_json = include_str!("../../block_vectors.json");

        assert!(
            vectors_json.trim() != "[]" && !vectors_json.is_empty(),
            "Test vectors not populated - block_vectors.json is empty. \
             Generate vectors from C# node before running this test."
        );

        // Parse and validate each block vector
        let vectors: Vec<serde_json::Value> =
            serde_json::from_str(vectors_json).expect("Failed to parse test vectors");

        for (i, vector) in vectors.iter().enumerate() {
            println!("Validating block vector {}", i);
            // TODO: Implement actual validation against C# behavior
            assert!(vector.is_object(), "Vector {} should be an object", i);
        }
    }

    #[test]
    #[ignore = "test vectors not populated - genesis block hex is empty"]
    fn test_genesis_block_validation() {
        // Minimal test for genesis block structure
        let genesis_hex = ""; // TODO: Add from C# node

        assert!(
            !genesis_hex.is_empty(),
            "Genesis block hex not populated. \
             Export genesis block from C# node before running this test."
        );

        let bytes = hex::decode(genesis_hex).expect("Invalid hex");
        // TODO: Use proper deserialization method when available
        let _block = bytes; // Placeholder

        // TODO: Uncomment when deserialization is available
        // assert_eq!(block.index(), 0);
        // assert_eq!(block.prev_hash(), neo_primitives::UInt256::zero());
    }
}
