use neo_cryptography::{hash, ecdsa::ECDsa, base58};

fn main() {
    println!("=== Neo Cryptography C# Compatibility Test ===\n");
    
    // Test hash functions with known vectors
    test_hash_functions();
    
    // Test ECDSA operations
    test_ecdsa_operations();
    
    // Test Base58 encoding
    test_base58_operations();
    
    println!("=== All tests completed ===");
}

fn test_hash_functions() {
    println!("--- Hash Function Tests ---");
    
    // SHA256 test vectors (from Bitcoin/C# Neo)
    let test_cases = vec![
        ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        ("Neo", "effee861f3433baac2d48e5b422c771dfb3762fb096a4aa9a8ba49eb6e7d7c27"),
    ];
    
    for (input, expected) in test_cases {
        let result = hash::sha256(input.as_bytes());
        let result_hex = hex::encode(result);
        println!("SHA256('{}') = {}", input, result_hex);
        println!("Expected:      {}", expected);
        println!("Match: {}\n", result_hex == expected);
    }
    
    // RIPEMD160 test vectors
    let ripemd_cases = vec![
        ("", "9c1185a5c5e9fc54612808977ee8f548b2258d31"),
        ("abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
    ];
    
    for (input, expected) in ripemd_cases {
        let result = hash::ripemd160(input.as_bytes());
        let result_hex = hex::encode(result);
        println!("RIPEMD160('{}') = {}", input, result_hex);
        println!("Expected:        {}", expected);
        println!("Match: {}\n", result_hex == expected);
    }
}

fn test_ecdsa_operations() {
    println!("--- ECDSA Operations Tests ---");
    
    // Generate a test keypair
    let private_key = ECDsa::generate_private_key();
    println!("Generated private key: {}", hex::encode(private_key));
    
    // Derive public key
    let public_key = ECDsa::derive_public_key(&private_key).unwrap();
    println!("Derived public key (uncompressed): {}", hex::encode(&public_key));
    
    // Compress public key
    let compressed_key = ECDsa::compress_public_key(&public_key).unwrap();
    println!("Compressed public key: {}", hex::encode(&compressed_key));
    
    // Test signature
    let message = b"Neo blockchain test message";
    let signature = ECDsa::sign_neo_format(message, &private_key).unwrap();
    println!("Signature: {}", hex::encode(signature));
    
    // Verify signature
    let is_valid = ECDsa::verify_neo_format(message, &signature, &public_key).unwrap();
    println!("Signature valid: {}\n", is_valid);
}

fn test_base58_operations() {
    println!("--- Base58 Operations Tests ---");
    
    // Test vectors from Bitcoin
    let test_cases = vec![
        (vec![], ""),
        (vec![0], "1"),
        (vec![0, 0], "11"),
        (vec![1, 2, 3], "Ldp"),
        (vec![255], "5Q"),
    ];
    
    for (input, expected) in test_cases {
        let encoded = base58::encode(&input);
        println!("Base58({:?}) = '{}'", input, encoded);
        println!("Expected:       '{}'", expected);
        println!("Match: {}", encoded == expected);
        
        // Test round-trip
        if !encoded.is_empty() {
            let decoded = base58::decode(&encoded).unwrap();
            println!("Round-trip: {:?} -> {} -> {:?} = {}\n", 
                input, encoded, decoded, input == decoded);
        } else {
            println!();
        }
    }
    
    // Test Base58Check
    let test_data = vec![1, 2, 3, 4, 5];
    let encoded_check = base58::encode_check(&test_data);
    println!("Base58Check({:?}) = '{}'", test_data, encoded_check);
    
    let decoded_check = base58::decode_check(&encoded_check).unwrap();
    println!("Decoded: {:?}", decoded_check);
    println!("Round-trip success: {}\n", test_data == decoded_check);
}