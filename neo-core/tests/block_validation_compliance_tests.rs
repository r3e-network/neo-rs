//! Block validation protocol compliance tests
//!
//! These tests require real test vectors generated from a C# Neo node.
//! They remain explicitly ignored until the v3.10.1 C# vector artifact is
//! provisioned. They are not counted as protocol coverage while the fixture is
//! empty. To enable them, provide `block_vectors.json` and the genesis block
//! hex generated from a pinned Neo v3.10.1 node.

#[cfg(test)]
mod tests {

    #[test]
    #[ignore = "v3.10.1 C# block_vectors.json fixture is not provisioned"]
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
            // Full validation runs once the v3.10.1 vector decoder is wired up.
            assert!(vector.is_object(), "Vector {} should be an object", i);
        }
    }

    #[test]
    #[ignore = "v3.10.1 C# genesis block fixture is not provisioned"]
    fn test_genesis_block_validation() {
        // Genesis-hex fixture from a v3.10.1 C# node is not provisioned yet;
        // the full deserialization assertion is enabled once that fixture exists.
        let genesis_hex = ""; // Provided by the v3.10.1 C# genesis fixture.

        assert!(
            !genesis_hex.is_empty(),
            "Genesis block hex not populated. \
             Export genesis block from C# node before running this test."
        );

        let _bytes = hex::decode(genesis_hex).expect("Invalid hex");
        // Decode via Block::deserialize and assert index/hash once fixture is wired.
    }
}
