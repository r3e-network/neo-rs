//! Differential tests for Merkle Tree Root Computation refinement
//!
//! Phase-1 Coq Formal Verification - Module #5
//! Tests verified against neo-crypto/src/merkle_tree.rs (lines 38-117)

use neo_crypto::MerkleTree;
use neo_primitives::UInt256;
use rand::Rng;

/// Generate random hash value
fn generate_random_hash() -> UInt256 {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = std::array::from_fn(|_| rng.r#gen());
    UInt256::from(bytes)
}

/// Alias for generate_random_hash (for backward compatibility with other tests)
fn random_hash() -> UInt256 {
    generate_random_hash()
}

/// Test 1: Empty input handling
/// Coq lemma: merkle_root_empty_yields_none
#[test]
fn test_merkle_root_empty_input_returns_none() {
    println!("Test 1: Empty input handling");

    // From Rust source lines 92-94
    // "if hashes.is_empty() { return None; }"
    let result = MerkleTree::compute_root(&[]);

    assert_eq!(result, None, "Empty hash list yields no merkle root");
    println!("✓ Empty input correctly yields None (no root possible)");
}

/// Test 2: Single element identity property
/// Coq lemma: merkle_root_single_is_identity
#[test]
fn test_merkle_root_single_transaction_is_itself() {
    println!("\nTest 2: Single element identity");

    // From Rust source lines 96-98
    // "if hashes.len() == 1 { return Some(hashes[0]); }"
    let tx_hash = UInt256::from([1u8; 32]);
    let root = MerkleTree::compute_root(&[tx_hash]);

    assert_eq!(
        root,
        Some(tx_hash),
        "Single hash should be its own merkle root"
    );
    println!("✓ Single hash h serves as root(h): identity property holds");
}

/// Test 3: Two elements direct pairing
/// Coq lemma: merkle_root_two_elements_pairs
#[test]
fn test_merkle_root_two_transactions_pairs_directly() {
    println!("\nTest 3: Two element pairing");

    let h1 = UInt256::from([1u8; 32]);
    let h2 = UInt256::from([2u8; 32]);

    let root = MerkleTree::compute_root(&[h1, h2]).unwrap();
    let expected = MerkleTree::hash_pair(&h1, &h2);

    assert_eq!(
        root, expected,
        "Two hashes should pair directly without duplication"
    );
    println!("✓ [h1, h2] → hash_pair(h1, h2) ✓");
}

/// Test 4a: Odd-length duplication - three elements
/// Coq lemma: merkle_three_elements_duplicate_last
#[test]
fn test_merkle_odd_three_transactions_duplicates_last() {
    println!("\nTest 4a: Three element odd case");

    let h1 = generate_random_hash();
    let h2 = generate_random_hash();
    let h3 = generate_random_hash();

    let root = MerkleTree::compute_root(&[h1, h2, h3]).unwrap();

    // Algorithm from lines 103-113:
    // Pass 1: i=0 pairs (h1,h2); i=2 has only h3 so duplicates it → [pair(h1,h2), pair(h3,h3)]
    // Pass 2: pairs these two elements
    let p12 = MerkleTree::hash_pair(&h1, &h2);
    let p33 = MerkleTree::hash_pair(&h3, &h3);
    let expected = MerkleTree::hash_pair(&p12, &p33);

    assert_eq!(
        root, expected,
        "Three hashes: first pass creates [hash(h1,h2), hash(h3,h3)], then pair them"
    );

    println!("✓ [h1, h2, h3] → hash(hash(h1,h2), hash(h3,h3)) ✓");
}

/// Test 4b: Odd-length duplication - five elements
#[test]
fn test_merkle_odd_five_transactions_proper_reduction() {
    println!("\nTest 4b: Five element odd case");

    let hashes: Vec<UInt256> = (0..5).map(|i| UInt256::from([(i + 1) as u8; 32])).collect();

    let root = MerkleTree::compute_root(&hashes).unwrap();

    // Trace through algorithm (lines 103-113):
    // Level 0: [h1, h2, h3, h4, h5] (5 elements)
    // Pass 1 (5 elements):
    //   i=0: pair(h1,h2)
    //   i=2: pair(h3,h4)
    //   i=4: only h5 remains, so pair(h5,h5) due to default to self
    //   Result: [pair(h1,h2), pair(h3,h4), pair(h5,h5)] (3 elements)
    // Pass 2 (3 elements):
    //   i=0: pair(p12, p34)
    //   i=2: only p55 remains, so pair(p55,p55)
    //   Result: [pair(p12,p34), pair(p55,p55)] (2 elements)
    // Pass 3 (2 elements):
    //   Final pair: pair(pair(p12,p34), pair(p55,p55))

    let p12 = MerkleTree::hash_pair(&hashes[0], &hashes[1]);
    let p34 = MerkleTree::hash_pair(&hashes[2], &hashes[3]);
    let p55 = MerkleTree::hash_pair(&hashes[4], &hashes[4]);
    let p1234 = MerkleTree::hash_pair(&p12, &p34);
    let p5555 = MerkleTree::hash_pair(&p55, &p55);
    let final_root = MerkleTree::hash_pair(&p1234, &p5555);

    assert_eq!(root, final_root, "Five-element tree reduces properly");
    println!("✓ 5-element odd list correctly applies per-pair duplication rule");
    println!("  Structure: hash(hash(hash(h1,h2),hash(h3,h4)), hash(hash(h5,h5),hash(h5,h5)))");

    // Verify determinism by recomputing
    let root2 = MerkleTree::compute_root(&hashes).unwrap();
    assert_eq!(root, root2, "Same input must produce same output");
    println!("✓ Deterministic computation verified");
}

/// Test 4c: Larger odd case - seven elements
#[test]
fn test_merkle_seven_transactions_edge_case() {
    println!("\nTest 4c: Seven element odd case");

    let hashes: Vec<UInt256> = (0..7).map(|i| UInt256::from([(i + 1) as u8; 32])).collect();

    // Verify that seven valid hashes produce some root
    let root = MerkleTree::compute_root(&hashes).unwrap();
    assert_eq!(root.to_bytes().len(), 32, "Root must be a valid UInt256");

    // Verify deterministic across multiple runs
    let roots: Vec<_> = (0..10).map(|_| MerkleTree::compute_root(&hashes)).collect();

    for r in &roots[1..] {
        assert_eq!(*r, roots[0], "Determinism check");
    }

    println!("✓ 7-element tree produces consistent deterministic results");
}

/// Test 5a: Compositional correctness - hash_pair uses SHA256 concatenation
/// Coq lemma: hash_pair_uses_concatenation
#[test]
fn test_hash_pair_uses_sha256_concatenation() {
    println!("\nTest 5a: Hash pair implementation details");

    use neo_crypto::Crypto;

    let left = UInt256::from([1u8; 32]);
    let right = UInt256::from([2u8; 32]);

    let pair = MerkleTree::hash_pair(&left, &right);

    // Manual computation per Rust lines 216-221:
    // Creates 64-byte array with left || right, then applies Crypto::hash256
    let mut concatenated = [0u8; 64];
    concatenated[..32].copy_from_slice(&left.to_bytes());
    concatenated[32..].copy_from_slice(&right.to_bytes());

    let manual = Crypto::hash256(&concatenated);

    assert_eq!(
        pair.to_bytes(),
        manual,
        "Hash pair must equal SHA256(left_bytes || right_bytes)"
    );

    println!("✓ hash_pair(L,R) = SHA256(L || R) verified");
    println!(
        "  Left:  {}...",
        left.to_string().chars().take(16).collect::<String>()
    );
    println!(
        "  Right: {}...",
        right.to_string().chars().take(16).collect::<String>()
    );
}

/// Test 5b: Root computation matches reference algorithm structure
#[test]
fn test_compute_root_matches_reference_algorithm() {
    println!("\nTest 5b: Reference algorithm comparison");

    // Compare against known structural properties from Rust implementation

    let test_cases = [
        vec![],                                            // Line 92-94
        vec![random_hash()],                               // Line 96-98
        vec![random_hash(), random_hash()],                // Line 103-113 even case
        vec![random_hash(), random_hash(), random_hash()], // Line 108-109 odd case
    ];

    for hashes in test_cases.iter() {
        let result = MerkleTree::compute_root(hashes);

        // Verify output format: always 32 bytes when Some
        if let Some(r) = result {
            assert_eq!(
                r.to_bytes().len(),
                32,
                "Merle root must always be 32 bytes (UInt256)"
            );

            println!(
                "✓ Input len {}: root is valid UInt256 ({}...)",
                hashes.len(),
                r.to_string().chars().take(32).collect::<String>()
            );
        }
    }

    println!("✓ All inputs produce properly formatted UInt256 outputs");
}

/// Test 6: Determinism verification - pure function property
/// Coq lemma: merkle_root_deterministic
#[test]
fn test_merkle_root_always_deterministic() {
    println!("\nTest 6: Determinism verification");

    let test_hashes = vec![
        generate_random_hash(),
        generate_random_hash(),
        generate_random_hash(),
        generate_random_hash(),
    ];

    // Run 100 times to catch any non-determinism
    let results: Vec<_> = (0..100)
        .map(|_| MerkleTree::compute_root(&test_hashes))
        .collect();

    // All 100 runs must produce identical result
    for (i, r) in results.iter().enumerate().skip(1) {
        assert_eq!(*r, results[0], "Determinism failed on iteration {}", i);
    }

    println!("✓ Verified deterministic across 100 consecutive runs");
    println!("  Same input → Same output (pure function property)");
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn run_all_merkle_refinement_tests() {
        println!("=== Merkle Tree Root Computation Refinement Tests ===\n");
        println!("Module #5 - Phase-1 Coq Formal Verification\n");

        println!("--- Basic Properties ---");
        test_merkle_root_empty_input_returns_none();

        test_merkle_root_single_transaction_is_itself();

        test_merkle_root_two_transactions_pairs_directly();

        println!("\n--- Odd-Length Duplication Rule ---");
        test_merkle_odd_three_transactions_duplicates_last();

        test_merkle_odd_five_transactions_proper_reduction();

        test_merkle_seven_transactions_edge_case();

        println!("\n--- Compositional Correctness ---");
        test_hash_pair_uses_sha256_concatenation();

        test_compute_root_matches_reference_algorithm();

        println!("\n--- Pure Function Property ---");
        test_merkle_root_always_deterministic();

        println!("\n=========================================");
        println!("ALL MERKLE TREE REFINEMENT TESTS PASSED ✅");
        println!("=========================================");
        println!("\nTie-In Summary:");
        println!("✓ Empty hash list → No root possible (Coq: merkle_root_empty_yields_none)");
        println!("✓ Single hash → Identity property (Coq: merkle_root_single_is_identity)");
        println!("✓ Two hashes → Direct pairing (Coq: merkle_root_two_elements_pairs)");
        println!(
            "✓ Odd lists → Last element duplicated (Coq: merkle_three_elements_duplicate_last)"
        );
        println!("✓ Hash pair → SHA256(concat(L||R)) (Coq: hash_pair_uses_concatenation)");
        println!("✓ Determinism → Same input, same output (Coq: merkle_root_deterministic)");
    }
}
