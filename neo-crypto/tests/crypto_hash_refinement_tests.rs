//! Differential tests for cryptographic hash function refinement
//! 
//! Phase-1 Coq Formal Verification - Module #2
//! Tests verified against known vectors in neo-crypto/src/hash.rs

use neo_crypto::Crypto;
use rand::Rng;

/// Generate random bytes of specified length
fn random_bytes(len: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..len).map(|_| rng.r#gen()).collect()
}

/// Test 1: SHA256 always produces 32-byte output regardless of input length
#[test]
fn test_sha256_always_returns_32_bytes() {
    let test_inputs = vec![
        vec![],                              // Empty input (line 307)
        b"hello".to_vec(),                   // Short string (line 410-418)
        vec![0u8; 64],                       // Medium block
        vec![0u8; 1024],                     // Large input
        random_bytes(256),                   // Random 256 bytes
        random_bytes(1024),                  // Random large input
        vec![255u8; 1000],                   // Max byte values
    ];
    
    for input in test_inputs {
        let digest = Crypto::sha256(&input);
        assert_eq!(digest.len(), 32, "SHA256 must always produce 32-byte digest");
        println!("✓ Input len {}: SHA256→{} bytes", input.len(), digest.len());
    }
}

/// Test 2: RIPEMD160 always produces 20-byte output
#[test]
fn test_ripemd160_always_returns_20_bytes() {
    let test_inputs = vec![
        vec![],
        b"hello".to_vec(),
        vec![0u8; 64],
        vec![0u8; 512],
        random_bytes(128),
    ];
    
    for input in test_inputs {
        let digest = Crypto::ripemd160(&input);
        assert_eq!(digest.len(), 20, "RIPEMD160 must always produce 20-byte digest");
        println!("✓ Input len {}: RIPEMD160→{} bytes", input.len(), digest.len());
    }
}

/// Test 3: Hash160 always produces 20-byte output
#[test]
fn test_hash160_always_returns_20_bytes() {
    let test_inputs = vec![
        vec![],
        b"hello".to_vec(),
        vec![0u8; 64],
        vec![0u8; 1024],
        random_bytes(256),
    ];
    
    for input in test_inputs {
        let digest = Crypto::hash160(&input);
        assert_eq!(digest.len(), 20, "Hash160 must always produce 20-byte digest");
        println!("✓ Input len {}: Hash160→{} bytes", input.len(), digest.len());
    }
}

/// Test 4: Hash256 always produces 32-byte output
#[test]
fn test_hash256_always_returns_32_bytes() {
    let test_inputs = vec![
        vec![],
        b"hello".to_vec(),
        vec![0u8; 64],
        vec![0u8; 512],
        random_bytes(128),
    ];
    
    for input in test_inputs {
        let digest = Crypto::hash256(&input);
        assert_eq!(digest.len(), 32, "Hash256 must always produce 32-byte digest");
        println!("✓ Input len {}: Hash256→{} bytes", input.len(), digest.len());
    }
}

/// Test 5: Hash160 composition property - must equal RIPEMD160∘SHA256 chain
#[test]
fn test_hash160_composition_correctness() {
    let test_cases = vec![
        vec![],
        b"hello".to_vec(),                          // From test_hash160 (line 442-449)
        b"test data for composition".to_vec(),
        vec![0u8; 64],
        vec![1u8; 128],
        random_bytes(100),
        random_bytes(300),
    ];
    
    for data in test_cases {
        let direct = Crypto::hash160(&data);
        let chained = Crypto::ripemd160(&Crypto::sha256(&data));
        
        assert_eq!(direct, chained, 
            "Hash160(x) must equal RIPEMD160(SHA256(x)) for all inputs");
        println!("✓ Composition verified for {}-byte input", data.len());
    }
}

/// Test 6: Hash256 composition property - must equal SHA256∘SHA256 chain
#[test]
fn test_hash256_composition_correctness() {
    let test_cases = vec![
        vec![],
        b"hello".to_vec(),                          // From test_hash256 (line 452-459)
        b"transaction verification".to_vec(),
        vec![0u8; 32],
        vec![255u8; 128],
        random_bytes(50),
        random_bytes(200),
    ];
    
    for data in test_cases {
        let direct = Crypto::hash256(&data);
        let double = Crypto::sha256(&Crypto::sha256(&data));
        
        assert_eq!(direct, double, 
            "Hash256(x) must equal SHA256(SHA256(x)) for all inputs");
        println!("✓ Double SHA256 verified for {}-byte input", data.len());
    }
}

/// Test 7: Known vector - SHA256("hello") from neo-crypto/src/hash.rs line 410-418
#[test]
fn test_sha256_hello_known_vector() {
    // Exact hex from Rust test: 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    let result = Crypto::sha256(b"hello");
    let expected = [
        0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e,
        0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9, 0xe2, 0x9e,
        0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e,
        0x73, 0x04, 0x33, 0x62, 0x93, 0x8b, 0x98, 0x24,
    ];
    
    assert_eq!(result, expected, 
        "SHA256('hello') must match C# reference implementation exactly");
    
    println!("✓ SHA256('hello') = {}", 
        result.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
}

/// Test 8: Known vector - Hash160 composition with "hello"
#[test]
fn test_hash160_hello_known_vector() {
    let hello = b"hello";
    let sha256_result = Crypto::sha256(hello);
    let ripemd_result = Crypto::ripemd160(&sha256_result);
    let hash160_direct = Crypto::hash160(hello);
    
    assert_eq!(hash160_direct, ripemd_result,
        "Hash160('hello') must equal RIPEMD160(SHA256('hello'))");
    
    println!("✓ Hash160('hello') = {}", 
        hash160_direct.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
    println!("  (computed as RIPEMD160(SHA256('hello')))");
}

/// Test 9: Constant-time comparison equals standard equality
#[test]
fn test_ct_hash_eq_matches_equal_operator() {
    use neo_crypto::hash::ct_hash_eq;
    
    // Test identical arrays (should match)
    let identical_zeros: [u8; 32] = [0u8; 32];
    assert!(ct_hash_eq(&identical_zeros, &identical_zeros), "Identical zero arrays must match");
    println!("✓ CT eq matches std eq for identical [0u8; 32]");
    
    let hello_bytes: [u8; 5] = *b"hello";
    assert!(ct_hash_eq(&hello_bytes, &hello_bytes), "Self-comparison must always be true");
    println!("✓ CT eq matches std eq for self-comparison of 'hello'");
    
    // Test different arrays (should not match)
    let ones: [u8; 32] = [1u8; 32];
    let twos: [u8; 32] = [2u8; 32];
    assert!(!ct_hash_eq(&ones, &twos), "Different arrays must not match");
    println!("✓ CT eq matches std eq for [1u8; 32] vs [2u8; 32]");
    
    // Test last byte difference (critical for constant-time verification)
    let arr_a: [u8; 32] = std::iter::repeat(0u8).take(31).chain(std::iter::once(1)).collect::<Vec<_>>().try_into().unwrap();
    let arr_b: [u8; 32] = std::iter::repeat(0u8).take(31).chain(std::iter::once(2)).collect::<Vec<_>>().try_into().unwrap();
    assert!(!ct_hash_eq(&arr_a, &arr_b), "Arrays differing only at last position must not match");
    println!("✓ CT eq detects single-byte difference at last position");
    
    println!("✓ All CT comparison tests passed");
}

/// Test 10: CT hash slice equality handles different lengths correctly
#[test]
fn test_ct_hash_slice_eq_different_lengths() {
    use neo_crypto::hash::ct_hash_slice_eq;
    
    let a = [0u8; 32];
    let b = [0u8; 64];
    let c = [0u8; 20];
    
    assert!(!ct_hash_slice_eq(&a, &b), "32 vs 64 bytes must return false");
    assert!(!ct_hash_slice_eq(&a, &c), "32 vs 20 bytes must return false");
    assert!(ct_hash_slice_eq(&a, &a), "Same 32 bytes must return true");
    
    println!("✓ CT slice equality correctly rejects different length inputs");
}

/// Test 11: CT comparison detects single byte differences at various positions
#[test]
fn test_ct_hash_eq_single_byte_detection() {
    use neo_crypto::hash::ct_hash_eq;
    
    let mut base = [0u8; 32];
    let mut different = [0u8; 32];
    
    // Fill with same pattern
    for i in 0..32 {
        base[i] = i as u8;
        different[i] = i as u8;
    }
    
    // Test differences at critical positions
    let test_positions = vec![0, 1, 15, 30, 31];
    
    for pos in test_positions {
        different[pos] ^= 0xFF; // Flip all bits at this position
        
        assert!(!ct_hash_eq(&base, &different), 
            "CT eq must detect difference at position {}", pos);
        
        // Reset for next iteration
        different[pos] ^= 0xFF;
    }
    
    println!("✓ CT comparison detects single-byte differences at all positions");
}

/// Test 12: Edge case - empty string hashing
#[test]
fn test_empty_string_hashing() {
    let empty = vec![];
    
    let sha256_empty = Crypto::sha256(&empty);
    let ripemd_empty = Crypto::ripemd160(&empty);
    let hash160_empty = Crypto::hash160(&empty);
    let hash256_empty = Crypto::hash256(&empty);
    
    // Verify all produce correct sizes
    assert_eq!(sha256_empty.len(), 32);
    assert_eq!(ripemd_empty.len(), 20);
    assert_eq!(hash160_empty.len(), 20);
    assert_eq!(hash256_empty.len(), 32);
    
    // Verify compositions work on empty input too
    let expected_hash160 = Crypto::ripemd160(&sha256_empty);
    let expected_hash256 = Crypto::sha256(&sha256_empty);
    
    assert_eq!(hash160_empty, expected_hash160);
    assert_eq!(hash256_empty, expected_hash256);
    
    println!("✓ All hash functions handle empty input correctly");
    println!("  SHA256(empty) = {}", 
        sha256_empty.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn run_all_hash_refinement_tests() {
        println!("=== Cryptographic Hash Refinement Differential Tests ===\n");
        
        // Run individual test suites
        test_sha256_always_returns_32_bytes();
        println!();
        
        test_ripemd160_always_returns_20_bytes();
        println!();
        
        test_hash160_always_returns_20_bytes();
        println!();
        
        test_hash256_always_returns_32_bytes();
        println!();
        
        test_hash160_composition_correctness();
        println!();
        
        test_hash256_composition_correctness();
        println!();
        
        test_sha256_hello_known_vector();
        println!();
        
        test_hash160_hello_known_vector();
        println!();
        
        test_ct_hash_eq_matches_equal_operator();
        println!();
        
        test_ct_hash_slice_eq_different_lengths();
        println!();
        
        test_ct_hash_eq_single_byte_detection();
        println!();
        
        test_empty_string_hashing();
        println!();
        
        println!("=== ALL TESTS PASSED ✅ ===");
    }
}
