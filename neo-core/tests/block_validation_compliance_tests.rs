//! Block validation protocol compliance tests

#[cfg(test)]
mod tests {

    #[test]
    fn test_block_validation_vectors() {
        // Load test vectors generated from C# node
        let vectors_json = include_str!("../../block_vectors.json");

        if vectors_json.trim() == "[]" || vectors_json.is_empty() {
            println!("WARN: No test vectors available yet");
            return;
        }

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
    fn test_genesis_block_validation() {
        // Minimal test for genesis block structure
        let genesis_hex = ""; // TODO: Add from C# node

        if genesis_hex.is_empty() {
            println!("Skipping: genesis block hex not provided");
            return;
        }

        let bytes = hex::decode(genesis_hex).expect("Invalid hex");
        // TODO: Use proper deserialization method when available
        let _block = bytes; // Placeholder

        // TODO: Uncomment when deserialization is available
        // assert_eq!(block.index(), 0);
        // assert_eq!(block.prev_hash(), neo_primitives::UInt256::zero());
    }
}
