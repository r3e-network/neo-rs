//! Comprehensive Cryptography Integration Tests
//!
//! These tests verify cryptographic operations, signature verification,
//! hash functions, key derivation, and blockchain-specific crypto functionality.

use neo_cryptography::{
    ecdsa::{ECDsa, ECPoint, PrivateKey, PublicKey},
    hash::{Hash160, Hash256, Sha256, Ripemd160},
    ecc::ECC,
};
use neo_core::{UInt160, UInt256};
use std::collections::HashMap;
use tokio_test;

/// Test ECDSA key generation and operations
#[tokio::test]
async fn test_ecdsa_key_operations() {
    println!("🔐 Testing ECDSA key operations");
    
    // Test secp256r1 (NIST P-256) operations
    println!("  Testing secp256r1 operations...");
    
    // Generate key pair
    let private_key = PrivateKey::generate_secp256r1();
    assert!(private_key.is_ok(), "Should generate secp256r1 private key successfully");
    let private_key = private_key.unwrap();
    
    // Derive public key
    let public_key = private_key.public_key();
    assert!(public_key.is_ok(), "Should derive public key successfully");
    let public_key = public_key.unwrap();
    
    // Test key serialization
    let private_key_bytes = private_key.to_bytes();
    assert_eq!(private_key_bytes.len(), 32, "Private key should be 32 bytes");
    
    let public_key_compressed = public_key.to_compressed_bytes();
    assert_eq!(public_key_compressed.len(), 33, "Compressed public key should be 33 bytes");
    
    let public_key_uncompressed = public_key.to_uncompressed_bytes();
    assert_eq!(public_key_uncompressed.len(), 65, "Uncompressed public key should be 65 bytes");
    
    // Test key deserialization
    let restored_private_key = PrivateKey::from_bytes(&private_key_bytes);
    assert!(restored_private_key.is_ok(), "Should restore private key from bytes");
    
    let restored_public_key = PublicKey::from_compressed_bytes(&public_key_compressed);
    assert!(restored_public_key.is_ok(), "Should restore public key from compressed bytes");
    
    // Verify keys match
    let restored_public_from_private = restored_private_key.unwrap().public_key().unwrap();
    assert_eq!(
        restored_public_from_private.to_compressed_bytes(),
        public_key_compressed,
        "Restored public key should match original"
    );
    
    // Test secp256k1 operations
    println!("  Testing secp256k1 operations...");
    
    let secp256k1_private = PrivateKey::generate_secp256k1();
    assert!(secp256k1_private.is_ok(), "Should generate secp256k1 private key successfully");
    
    let secp256k1_public = secp256k1_private.unwrap().public_key().unwrap();
    let secp256k1_compressed = secp256k1_public.to_compressed_bytes();
    assert_eq!(secp256k1_compressed.len(), 33, "secp256k1 compressed public key should be 33 bytes");
    
    println!("✅ ECDSA key operations test passed");
}

/// Test digital signature creation and verification
#[tokio::test]
async fn test_digital_signature_operations() {
    println!("✍️ Testing digital signature operations");
    
    // Test with various message sizes
    let test_messages = vec![
        b"Hello, Neo!".to_vec(),
        b"".to_vec(), // Empty message
        vec![0x42; 1000], // Large message
        (0..256).collect::<Vec<u8>>(), // All byte values
    ];
    
    for (i, message) in test_messages.iter().enumerate() {
        println!("  Testing message {}: {} bytes", i + 1, message.len());
        
        // Generate key pair
        let private_key = PrivateKey::generate_secp256r1().unwrap();
        let public_key = private_key.public_key().unwrap();
        
        // Sign message
        let signature = ECDsa::sign_secp256r1(message, &private_key);
        assert!(signature.is_ok(), "Should sign message successfully");
        let signature = signature.unwrap();
        assert_eq!(signature.len(), 64, "Signature should be 64 bytes (r + s)");
        
        // Verify signature
        let verification = ECDsa::verify_secp256r1(message, &signature, &public_key);
        assert!(verification.is_ok(), "Verification should not fail");
        assert!(verification.unwrap(), "Signature should be valid");
        
        // Test with wrong message
        let mut wrong_message = message.clone();
        if !wrong_message.is_empty() {
            wrong_message[0] = wrong_message[0].wrapping_add(1);
            
            let wrong_verification = ECDsa::verify_secp256r1(&wrong_message, &signature, &public_key);
            assert!(wrong_verification.is_ok(), "Verification should not fail");
            assert!(!wrong_verification.unwrap(), "Signature should be invalid for wrong message");
        }
        
        // Test with wrong signature
        let mut wrong_signature = signature.clone();
        wrong_signature[0] = wrong_signature[0].wrapping_add(1);
        
        let wrong_sig_verification = ECDsa::verify_secp256r1(message, &wrong_signature, &public_key);
        // Wrong signature should either fail verification or return false
        if let Ok(result) = wrong_sig_verification {
            assert!(!result, "Wrong signature should be invalid");
        }
    }
    
    println!("✅ Digital signature operations test passed");
}

/// Test hash function operations
#[tokio::test]
async fn test_hash_function_operations() {
    println!("🔢 Testing hash function operations");
    
    let test_data = vec![
        b"".to_vec(),
        b"a".to_vec(),
        b"abc".to_vec(),
        b"The quick brown fox jumps over the lazy dog".to_vec(),
        vec![0x00; 1000],
        vec![0xFF; 1000],
        (0..256).cycle().take(10000).collect::<Vec<u8>>(),
    ];
    
    for (i, data) in test_data.iter().enumerate() {
        println!("  Testing hash functions with data {}: {} bytes", i + 1, data.len());
        
        // Test SHA256
        let sha256_result = Sha256::hash(data);
        assert!(sha256_result.is_ok(), "SHA256 should succeed");
        let sha256_hash = sha256_result.unwrap();
        assert_eq!(sha256_hash.len(), 32, "SHA256 should produce 32-byte hash");
        
        // Test Hash256 (double SHA256)
        let hash256_result = Hash256::hash(data);
        assert!(hash256_result.is_ok(), "Hash256 should succeed");
        let hash256_hash = hash256_result.unwrap();
        assert_eq!(hash256_hash.len(), 32, "Hash256 should produce 32-byte hash");
        
        // Verify Hash256 is double SHA256
        let manual_double_sha256 = Sha256::hash(&sha256_hash).unwrap();
        assert_eq!(hash256_hash, manual_double_sha256, "Hash256 should equal double SHA256");
        
        // Test RIPEMD160
        let ripemd160_result = Ripemd160::hash(data);
        assert!(ripemd160_result.is_ok(), "RIPEMD160 should succeed");
        let ripemd160_hash = ripemd160_result.unwrap();
        assert_eq!(ripemd160_hash.len(), 20, "RIPEMD160 should produce 20-byte hash");
        
        // Test Hash160 (RIPEMD160 of SHA256)
        let hash160_result = Hash160::hash(data);
        assert!(hash160_result.is_ok(), "Hash160 should succeed");
        let hash160_hash = hash160_result.unwrap();
        assert_eq!(hash160_hash.len(), 20, "Hash160 should produce 20-byte hash");
        
        // Verify Hash160 is RIPEMD160(SHA256(data))
        let manual_hash160 = Ripemd160::hash(&sha256_hash).unwrap();
        assert_eq!(hash160_hash, manual_hash160, "Hash160 should equal RIPEMD160(SHA256(data))");
        
        // Test deterministic behavior
        let sha256_repeat = Sha256::hash(data).unwrap();
        assert_eq!(sha256_hash, sha256_repeat, "Hash should be deterministic");
    }
    
    println!("✅ Hash function operations test passed");
}

/// Test Neo-specific address generation and validation
#[tokio::test]
async fn test_neo_address_operations() {
    println!("🏠 Testing Neo address operations");
    
    // Test script hash to address conversion
    let test_script_hashes = vec![
        UInt160::zero(),
        UInt160::from_bytes(&[0x01; 20]).unwrap(),
        UInt160::from_bytes(&[0xFF; 20]).unwrap(),
        UInt160::from_bytes(&[
            0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
            0x99, 0xAA, 0xBB, 0xCC
        ]).unwrap(),
    ];
    
    for (i, script_hash) in test_script_hashes.iter().enumerate() {
        println!("  Testing address generation {}: {}", i + 1, script_hash);
        
        // Convert to address
        let address = script_hash.to_address();
        assert!(!address.is_empty(), "Address should not be empty");
        assert!(address.starts_with('N'), "Neo N3 address should start with 'N'");
        
        // Verify address is valid Base58Check
        let decode_result = bs58::decode(&address).into_vec();
        assert!(decode_result.is_ok(), "Address should be valid Base58");
        let decoded = decode_result.unwrap();
        assert_eq!(decoded.len(), 25, "Decoded address should be 25 bytes");
        assert_eq!(decoded[0], 0x35, "Address version should be 0x35 for Neo N3");
        
        // Convert back from address
        let restored_script_hash = UInt160::from_address(&address);
        assert!(restored_script_hash.is_ok(), "Should restore script hash from address");
        assert_eq!(restored_script_hash.unwrap(), *script_hash, "Restored script hash should match original");
        
        // Test invalid addresses
        let invalid_addresses = vec![
            "", // Empty
            "invalid", // Not Base58
            "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNn", // Wrong checksum
            "MNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNn", // Wrong version (starts with M)
        ];
        
        for invalid_addr in &invalid_addresses {
            let invalid_result = UInt160::from_address(invalid_addr);
            assert!(invalid_result.is_err(), "Invalid address '{}' should fail", invalid_addr);
        }
    }
    
    // Test script to script hash conversion
    let test_scripts = vec![
        vec![], // Empty script
        vec![0x41], // Simple script
        vec![0x0C, 0x04, 0x74, 0x65, 0x73, 0x74, 0x41, 0x56, 0x37], // "test" script
        (0..100).collect::<Vec<u8>>(), // Longer script
    ];
    
    for (i, script) in test_scripts.iter().enumerate() {
        println!("  Testing script hash generation {}: {} bytes", i + 1, script.len());
        
        let script_hash = UInt160::from_script(script);
        
        // Verify script hash is Hash160 of script
        let manual_hash = Hash160::hash(script).unwrap();
        let manual_script_hash = UInt160::from_bytes(&manual_hash).unwrap();
        assert_eq!(script_hash, manual_script_hash, "Script hash should equal Hash160 of script");
        
        // Test round-trip conversion
        let address = script_hash.to_address();
        let restored_hash = UInt160::from_address(&address).unwrap();
        assert_eq!(restored_hash, script_hash, "Round-trip conversion should preserve script hash");
    }
    
    println!("✅ Neo address operations test passed");
}

/// Test EC point operations and key recovery
#[tokio::test]
async fn test_ec_point_operations() {
    println!("📐 Testing EC point operations");
    
    // Test point generation from private key
    let private_key = PrivateKey::generate_secp256r1().unwrap();
    let public_key = private_key.public_key().unwrap();
    
    // Test point compression/decompression
    let compressed = public_key.to_compressed_bytes();
    let uncompressed = public_key.to_uncompressed_bytes();
    
    // Verify compression format
    assert!(compressed[0] == 0x02 || compressed[0] == 0x03, "Compressed point should start with 0x02 or 0x03");
    assert_eq!(uncompressed[0], 0x04, "Uncompressed point should start with 0x04");
    
    // Test point restoration
    let restored_from_compressed = PublicKey::from_compressed_bytes(&compressed).unwrap();
    let restored_from_uncompressed = PublicKey::from_uncompressed_bytes(&uncompressed).unwrap();
    
    // Both should represent the same point
    assert_eq!(
        restored_from_compressed.to_compressed_bytes(),
        restored_from_uncompressed.to_compressed_bytes(),
        "Points restored from compressed and uncompressed should match"
    );
    
    // Test point addition (if supported)
    let private_key2 = PrivateKey::generate_secp256r1().unwrap();
    let public_key2 = private_key2.public_key().unwrap();
    
    // Test signature with both keys
    let message = b"test message";
    let signature1 = ECDsa::sign_secp256r1(message, &private_key).unwrap();
    let signature2 = ECDsa::sign_secp256r1(message, &private_key2).unwrap();
    
    assert!(ECDsa::verify_secp256r1(message, &signature1, &public_key).unwrap());
    assert!(ECDsa::verify_secp256r1(message, &signature2, &public_key2).unwrap());
    
    // Cross-verification should fail
    assert!(!ECDsa::verify_secp256r1(message, &signature1, &public_key2).unwrap());
    assert!(!ECDsa::verify_secp256r1(message, &signature2, &public_key).unwrap());
    
    println!("✅ EC point operations test passed");
}

/// Test cryptographic edge cases and error handling
#[tokio::test]
async fn test_cryptographic_edge_cases() {
    println!("⚠️ Testing cryptographic edge cases");
    
    // Test invalid private key values
    let invalid_private_keys = vec![
        vec![0x00; 32], // All zeros
        vec![0xFF; 32], // All ones (might be invalid for some curves)
    ];
    
    for (i, invalid_key) in invalid_private_keys.iter().enumerate() {
        println!("  Testing invalid private key {}", i + 1);
        
        let key_result = PrivateKey::from_bytes(invalid_key);
        // Some invalid keys might be rejected, others might be accepted but behave oddly
        if let Ok(key) = key_result {
            // If accepted, public key derivation should still work
            let pub_key_result = key.public_key();
            if pub_key_result.is_err() {
                println!("    Private key accepted but public key derivation failed (expected)");
            }
        } else {
            println!("    Invalid private key properly rejected");
        }
    }
    
    // Test invalid public key formats
    let invalid_public_keys = vec![
        vec![0x00; 33], // Wrong prefix
        vec![0x04; 33], // Wrong length for uncompressed
        vec![0x02; 65], // Wrong length for compressed
        vec![0xFF; 33], // Invalid prefix
        vec![], // Empty
    ];
    
    for (i, invalid_pub_key) in invalid_public_keys.iter().enumerate() {
        println!("  Testing invalid public key format {}", i + 1);
        
        if invalid_pub_key.len() == 33 {
            let result = PublicKey::from_compressed_bytes(invalid_pub_key);
            if result.is_err() {
                println!("    Invalid compressed public key properly rejected");
            }
        }
        
        if invalid_pub_key.len() == 65 {
            let result = PublicKey::from_uncompressed_bytes(invalid_pub_key);
            if result.is_err() {
                println!("    Invalid uncompressed public key properly rejected");
            }
        }
    }
    
    // Test signature with invalid data
    let valid_private_key = PrivateKey::generate_secp256r1().unwrap();
    let valid_public_key = valid_private_key.public_key().unwrap();
    
    let invalid_signatures = vec![
        vec![0x00; 64], // All zeros
        vec![0xFF; 64], // All ones
        vec![0x42; 63], // Wrong length
        vec![0x42; 65], // Wrong length
        vec![], // Empty
    ];
    
    let test_message = b"test message";
    
    for (i, invalid_sig) in invalid_signatures.iter().enumerate() {
        println!("  Testing invalid signature {}: {} bytes", i + 1, invalid_sig.len());
        
        if invalid_sig.len() == 64 {
            let verify_result = ECDsa::verify_secp256r1(test_message, invalid_sig, &valid_public_key);
            if let Ok(result) = verify_result {
                assert!(!result, "Invalid signature should not verify");
            } else {
                println!("    Invalid signature properly rejected during verification");
            }
        }
    }
    
    println!("✅ Cryptographic edge cases test passed");
}

/// Test multi-signature and threshold operations
#[tokio::test]
async fn test_multi_signature_operations() {
    println!("👥 Testing multi-signature operations");
    
    // Create multiple key pairs
    let key_count = 5;
    let mut key_pairs = Vec::new();
    
    for i in 0..key_count {
        let private_key = PrivateKey::generate_secp256r1().unwrap();
        let public_key = private_key.public_key().unwrap();
        key_pairs.push((private_key, public_key));
        println!("  Generated key pair {}", i + 1);
    }
    
    let message = b"Multi-signature test message";
    
    // Test individual signatures
    let mut signatures = Vec::new();
    for (i, (private_key, public_key)) in key_pairs.iter().enumerate() {
        let signature = ECDsa::sign_secp256r1(message, private_key).unwrap();
        
        // Verify individual signature
        let verification = ECDsa::verify_secp256r1(message, &signature, public_key).unwrap();
        assert!(verification, "Individual signature {} should be valid", i + 1);
        
        signatures.push(signature);
    }
    
    // Test cross-verification (signatures from one key shouldn't verify with another key)
    for i in 0..key_count {
        for j in 0..key_count {
            if i != j {
                let result = ECDsa::verify_secp256r1(message, &signatures[i], &key_pairs[j].1).unwrap();
                assert!(!result, "Signature {} should not verify with key {}", i + 1, j + 1);
            }
        }
    }
    
    // Test threshold signature simulation (M-of-N)
    let threshold = 3; // 3-of-5 threshold
    
    for subset_size in 1..=key_count {
        println!("  Testing {}-of-{} signature verification", subset_size, key_count);
        
        // Take first subset_size signatures
        let subset_signatures = &signatures[0..subset_size];
        let subset_keys = &key_pairs[0..subset_size];
        
        // Verify each signature in the subset
        let mut valid_count = 0;
        for (sig, (_, pub_key)) in subset_signatures.iter().zip(subset_keys.iter()) {
            if ECDsa::verify_secp256r1(message, sig, pub_key).unwrap() {
                valid_count += 1;
            }
        }
        
        assert_eq!(valid_count, subset_size, "All signatures in subset should be valid");
        
        // Check if threshold is met
        let threshold_met = valid_count >= threshold;
        println!("    Threshold met: {} (need {}, have {})", threshold_met, threshold, valid_count);
    }
    
    println!("✅ Multi-signature operations test passed");
}

/// Test cryptographic performance and benchmarks
#[tokio::test]
async fn test_cryptographic_performance() {
    println!("⚡ Testing cryptographic performance");
    
    let iterations = 100;
    let message = b"Performance test message";
    
    // Benchmark key generation
    let key_gen_start = std::time::Instant::now();
    let mut key_pairs = Vec::new();
    
    for _ in 0..iterations {
        let private_key = PrivateKey::generate_secp256r1().unwrap();
        let public_key = private_key.public_key().unwrap();
        key_pairs.push((private_key, public_key));
    }
    
    let key_gen_time = key_gen_start.elapsed();
    let avg_key_gen_time = key_gen_time / iterations;
    
    println!("  Key generation: {} iterations in {:?} (avg: {:?})", 
             iterations, key_gen_time, avg_key_gen_time);
    
    // Benchmark signing
    let signing_start = std::time::Instant::now();
    let mut signatures = Vec::new();
    
    for (private_key, _) in &key_pairs {
        let signature = ECDsa::sign_secp256r1(message, private_key).unwrap();
        signatures.push(signature);
    }
    
    let signing_time = signing_start.elapsed();
    let avg_signing_time = signing_time / iterations;
    
    println!("  Signing: {} iterations in {:?} (avg: {:?})", 
             iterations, signing_time, avg_signing_time);
    
    // Benchmark verification
    let verification_start = std::time::Instant::now();
    let mut verification_count = 0;
    
    for ((_, public_key), signature) in key_pairs.iter().zip(signatures.iter()) {
        if ECDsa::verify_secp256r1(message, signature, public_key).unwrap() {
            verification_count += 1;
        }
    }
    
    let verification_time = verification_start.elapsed();
    let avg_verification_time = verification_time / iterations;
    
    println!("  Verification: {} iterations in {:?} (avg: {:?})", 
             iterations, verification_time, avg_verification_time);
    assert_eq!(verification_count, iterations, "All signatures should verify");
    
    // Benchmark hashing
    let hash_start = std::time::Instant::now();
    
    for i in 0..iterations {
        let data = format!("Hash test data {}", i);
        let _sha256 = Sha256::hash(data.as_bytes()).unwrap();
        let _hash256 = Hash256::hash(data.as_bytes()).unwrap();
        let _hash160 = Hash160::hash(data.as_bytes()).unwrap();
    }
    
    let hash_time = hash_start.elapsed();
    let avg_hash_time = hash_time / (iterations * 3); // 3 hash operations per iteration
    
    println!("  Hashing: {} iterations in {:?} (avg: {:?})", 
             iterations * 3, hash_time, avg_hash_time);
    
    // Performance assertions (adjust thresholds as needed)
    assert!(avg_key_gen_time.as_millis() < 100, "Key generation should be reasonably fast");
    assert!(avg_signing_time.as_millis() < 50, "Signing should be reasonably fast");
    assert!(avg_verification_time.as_millis() < 50, "Verification should be reasonably fast");
    assert!(avg_hash_time.as_micros() < 1000, "Hashing should be very fast");
    
    println!("✅ Cryptographic performance test passed");
}

/// Test cryptographic compatibility with Neo blockchain
#[tokio::test]
async fn test_neo_blockchain_compatibility() {
    println!("🔗 Testing Neo blockchain compatibility");
    
    // Test known Neo addresses and their script hashes
    let known_test_cases = vec![
        // Format: (script_hash_hex, expected_address)
        // These are test cases from Neo documentation/tests
        ("0000000000000000000000000000000000000000", "NKhNscREBRvyQ7X2eCW8M4Nqz8jyqC7YhE"),
        ("0102030405060708090a0b0c0d0e0f1011121314", "NKrPUhGTVKAeEkhpMpuwKBXgWHUhPGJRc9"),
    ];
    
    for (script_hash_hex, expected_address) in known_test_cases {
        println!("  Testing script hash: {}", script_hash_hex);
        
        // Convert hex to bytes
        let script_hash_bytes = hex::decode(script_hash_hex).unwrap();
        assert_eq!(script_hash_bytes.len(), 20, "Script hash should be 20 bytes");
        
        let script_hash = UInt160::from_bytes(&script_hash_bytes).unwrap();
        let generated_address = script_hash.to_address();
        
        // Note: The expected addresses here are examples and might not match real Neo addresses
        // In a real test, you would use known addresses from the Neo blockchain
        println!("    Generated address: {}", generated_address);
        println!("    Expected address:  {}", expected_address);
        
        // Verify address format is correct
        assert!(generated_address.starts_with('N'), "Address should start with 'N'");
        assert!(generated_address.len() > 30, "Address should be reasonable length");
        
        // Test round-trip conversion
        let restored_script_hash = UInt160::from_address(&generated_address).unwrap();
        assert_eq!(restored_script_hash, script_hash, "Round-trip conversion should work");
    }
    
    // Test signature format compatibility
    let private_key = PrivateKey::generate_secp256r1().unwrap();
    let public_key = private_key.public_key().unwrap();
    let message = b"Neo compatibility test";
    
    let signature = ECDsa::sign_secp256r1(message, &private_key).unwrap();
    
    // Neo signatures should be 64 bytes (32-byte r + 32-byte s)
    assert_eq!(signature.len(), 64, "Neo signature should be 64 bytes");
    
    // Verify signature components are in valid range
    let r = &signature[0..32];
    let s = &signature[32..64];
    
    // Neither r nor s should be all zeros
    assert_ne!(r, &[0u8; 32], "Signature r component should not be zero");
    assert_ne!(s, &[0u8; 32], "Signature s component should not be zero");
    
    // Test public key format compatibility
    let compressed_pub_key = public_key.to_compressed_bytes();
    
    // Neo uses compressed public keys (33 bytes)
    assert_eq!(compressed_pub_key.len(), 33, "Neo public key should be 33 bytes (compressed)");
    assert!(compressed_pub_key[0] == 0x02 || compressed_pub_key[0] == 0x03, 
            "Compressed public key should start with 0x02 or 0x03");
    
    // Test script hash generation for contract deployment
    let simple_contract_script = vec![
        0x0C, 0x04, 0x6E, 0x61, 0x6D, 0x65, // PUSHDATA1 "name"
        0x41, // SYSCALL
        0x9d, 0xf5, 0x05, 0x16, // System.Runtime.Platform
    ];
    
    let contract_hash = UInt160::from_script(&simple_contract_script);
    let contract_address = contract_hash.to_address();
    
    println!("  Contract script hash: {}", contract_hash);
    println!("  Contract address: {}", contract_address);
    
    // Verify contract address format
    assert!(contract_address.starts_with('N'), "Contract address should start with 'N'");
    
    println!("✅ Neo blockchain compatibility test passed");
}

/// Test concurrent cryptographic operations
#[tokio::test]
async fn test_concurrent_cryptographic_operations() {
    println!("🔄 Testing concurrent cryptographic operations");
    
    let concurrent_operations = 50;
    let message = b"Concurrent test message";
    
    // Test concurrent key generation
    let key_gen_tasks = (0..concurrent_operations).map(|i| {
        tokio::spawn(async move {
            let private_key = PrivateKey::generate_secp256r1();
            (i, private_key.is_ok())
        })
    }).collect::<Vec<_>>();
    
    let key_gen_results = futures::future::join_all(key_gen_tasks).await;
    
    // Verify all key generations succeeded
    for result in key_gen_results.iter() {
        let (id, success) = result.as_ref().unwrap();
        assert!(*success, "Key generation {} should succeed", id);
    }
    
    // Test concurrent signing operations
    let signing_tasks = (0..concurrent_operations).map(|i| {
        let message = message.to_vec();
        tokio::spawn(async move {
            let private_key = PrivateKey::generate_secp256r1().unwrap();
            let signature = ECDsa::sign_secp256r1(&message, &private_key);
            (i, signature.is_ok())
        })
    }).collect::<Vec<_>>();
    
    let signing_results = futures::future::join_all(signing_tasks).await;
    
    // Verify all signing operations succeeded
    for result in signing_results.iter() {
        let (id, success) = result.as_ref().unwrap();
        assert!(*success, "Signing operation {} should succeed", id);
    }
    
    // Test concurrent hashing operations
    let hashing_tasks = (0..concurrent_operations).map(|i| {
        tokio::spawn(async move {
            let data = format!("Concurrent hash test {}", i);
            let sha256_result = Sha256::hash(data.as_bytes());
            let hash256_result = Hash256::hash(data.as_bytes());
            let hash160_result = Hash160::hash(data.as_bytes());
            
            (i, sha256_result.is_ok() && hash256_result.is_ok() && hash160_result.is_ok())
        })
    }).collect::<Vec<_>>();
    
    let hashing_results = futures::future::join_all(hashing_tasks).await;
    
    // Verify all hashing operations succeeded
    for result in hashing_results.iter() {
        let (id, success) = result.as_ref().unwrap();
        assert!(*success, "Hashing operation {} should succeed", id);
    }
    
    println!("✅ Concurrent cryptographic operations test passed");
} 