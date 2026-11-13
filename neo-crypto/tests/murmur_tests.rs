//! Comprehensive tests for Murmur hash functions

use neo_crypto::murmur::{murmur32, murmur128};

#[test]
fn test_murmur32_basic() {
    let data = b"hello";
    let hash = murmur32(data, 0);
    assert_ne!(hash, 0); // Should produce non-zero hash
}

#[test]
fn test_murmur32_deterministic() {
    let data = b"test data";
    let seed = 42;

    let hash1 = murmur32(data, seed);
    let hash2 = murmur32(data, seed);

    assert_eq!(hash1, hash2);
}

#[test]
fn test_murmur32_different_seeds() {
    let data = b"same data";

    let hash1 = murmur32(data, 0);
    let hash2 = murmur32(data, 1);

    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur32_different_data() {
    let seed = 0;

    let hash1 = murmur32(b"data1", seed);
    let hash2 = murmur32(b"data2", seed);

    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur32_empty_data() {
    let hash = murmur32(b"", 0);
    // Empty data with seed 0 may produce hash value 0
    // Just verify it runs without error
    let _ = hash;
}

#[test]
fn test_murmur32_single_byte() {
    let hash = murmur32(b"a", 0);
    assert_ne!(hash, 0);
}

#[test]
fn test_murmur32_large_data() {
    let data = vec![0xAB; 10000];
    let hash = murmur32(&data, 123);
    assert_ne!(hash, 0);
}

#[test]
fn test_murmur32_zero_seed() {
    let data = b"test";
    let hash1 = murmur32(data, 0);
    let hash2 = murmur32(data, 0);
    assert_eq!(hash1, hash2);
}

#[test]
fn test_murmur32_max_seed() {
    let data = b"test";
    let hash = murmur32(data, u32::MAX);
    assert_ne!(hash, 0);
}

#[test]
fn test_murmur128_basic() {
    let data = b"hello";
    let hash = murmur128(data, 0);
    assert_eq!(hash.len(), 16);
    assert_ne!(hash, [0u8; 16]);
}

#[test]
fn test_murmur128_deterministic() {
    let data = b"test data";
    let seed = 42;

    let hash1 = murmur128(data, seed);
    let hash2 = murmur128(data, seed);

    assert_eq!(hash1, hash2);
}

#[test]
fn test_murmur128_different_seeds() {
    let data = b"same data";

    let hash1 = murmur128(data, 0);
    let hash2 = murmur128(data, 1);

    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur128_different_data() {
    let seed = 0;

    let hash1 = murmur128(b"data1", seed);
    let hash2 = murmur128(b"data2", seed);

    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur128_empty_data() {
    let hash = murmur128(b"", 0);
    assert_eq!(hash.len(), 16);
}

#[test]
fn test_murmur128_single_byte() {
    let hash = murmur128(b"x", 0);
    assert_eq!(hash.len(), 16);
    assert_ne!(hash, [0u8; 16]);
}

#[test]
fn test_murmur128_known_vectors() {
    // Test with known vectors from the test module
    let test_cases = vec![
        (
            b"hello".as_slice(),
            123u32,
            "0bc59d0ad25fde2982ed65af61227a0e",
        ),
        (
            b"world".as_slice(),
            123u32,
            "3d3810fed480472bd214a14023bb407f",
        ),
        (
            b"hello world".as_slice(),
            123u32,
            "e0a0632d4f51302c55e3b3e48d28795d",
        ),
    ];

    for (input, seed, expected) in test_cases {
        let hash = murmur128(input, seed);
        assert_eq!(hex::encode(hash), expected);
    }
}

#[test]
fn test_murmur128_binary_data() {
    let data = vec![0x00, 0xFF, 0xAA, 0x55];
    let hash = murmur128(&data, 0);
    assert_eq!(hash.len(), 16);
}

#[test]
fn test_murmur128_large_data() {
    let data = vec![0x42; 10000];
    let hash = murmur128(&data, 999);
    assert_eq!(hash.len(), 16);
    assert_ne!(hash, [0u8; 16]);
}

#[test]
fn test_murmur128_incremental_data() {
    // Different lengths should produce different hashes
    let hash1 = murmur128(b"a", 0);
    let hash2 = murmur128(b"ab", 0);
    let hash3 = murmur128(b"abc", 0);

    assert_ne!(hash1, hash2);
    assert_ne!(hash2, hash3);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_murmur32_avalanche() {
    // Small change in input should produce different hash
    let hash1 = murmur32(b"test", 0);
    let hash2 = murmur32(b"Test", 0);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur128_avalanche() {
    // Small change in input should produce different hash
    let hash1 = murmur128(b"test", 0);
    let hash2 = murmur128(b"Test", 0);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur32_byte_order_sensitivity() {
    let hash1 = murmur32(&[0x01, 0x02], 0);
    let hash2 = murmur32(&[0x02, 0x01], 0);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur128_byte_order_sensitivity() {
    let hash1 = murmur128(&[0x01, 0x02], 0);
    let hash2 = murmur128(&[0x02, 0x01], 0);
    assert_ne!(hash1, hash2);
}

#[test]
fn test_murmur32_consecutive_values() {
    let seed = 100;
    let hashes: Vec<u32> = (0..10).map(|i| murmur32(&[i], seed)).collect();

    // All hashes should be different
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            assert_ne!(hashes[i], hashes[j]);
        }
    }
}

#[test]
fn test_murmur128_consecutive_values() {
    let seed = 100;
    let hashes: Vec<[u8; 16]> = (0..10).map(|i| murmur128(&[i], seed)).collect();

    // All hashes should be different
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            assert_ne!(hashes[i], hashes[j]);
        }
    }
}

#[test]
fn test_murmur32_with_neo_seed() {
    // Test with typical Neo usage patterns
    let data = b"script_hash_data";
    let hash = murmur32(data, 0xFBA4C795);
    assert_ne!(hash, 0);
}

#[test]
fn test_murmur128_hex_consistency() {
    let data = hex::decode("718f952132679baa9c5c2aa0d329fd2a").unwrap();
    let hash = murmur128(&data, 123);
    let expected = "9b4aa747ff0cf4e41b3d96251551c8ae";
    assert_eq!(hex::encode(hash), expected);
}

#[test]
fn test_murmur32_distribution() {
    // Test that different inputs produce well-distributed hashes
    let seed = 42;
    let mut hashes = Vec::new();

    for i in 0..100 {
        let data = format!("item_{i}");
        let hash = murmur32(data.as_bytes(), seed);
        hashes.push(hash);
    }

    // Check all hashes are unique (good distribution)
    hashes.sort_unstable();
    hashes.dedup();
    assert_eq!(hashes.len(), 100);
}

#[test]
fn test_murmur128_output_format() {
    let hash = murmur128(b"test", 0);

    // Verify it's 16 bytes (128 bits)
    assert_eq!(hash.len(), 16);

    // Verify we can encode to hex
    let hex_str = hex::encode(hash);
    assert_eq!(hex_str.len(), 32); // 16 bytes = 32 hex chars
}
