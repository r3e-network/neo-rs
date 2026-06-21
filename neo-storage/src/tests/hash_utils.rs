use super::*;

#[test]
fn test_default_seed() {
    assert_eq!(XxHash3::default_xx_hash3_seed(), 40343);
}

#[test]
fn test_xx_hash3_32_empty() {
    let hash1 = XxHash3::xx_hash3_32(&[], DEFAULT_XX_HASH3_SEED);
    let hash2 = XxHash3::xx_hash3_32(&[], DEFAULT_XX_HASH3_SEED);
    assert_eq!(hash1, hash2);
}

#[test]
fn test_xx_hash3_32_data() {
    let data = vec![0x01, 0x02, 0x03, 0x04];
    let hash1 = XxHash3::xx_hash3_32(&data, DEFAULT_XX_HASH3_SEED);
    let hash2 = XxHash3::xx_hash3_32(&data, DEFAULT_XX_HASH3_SEED);
    assert_eq!(hash1, hash2); // Same input, same hash
}

#[test]
fn test_hash_code_combine() {
    let hash = XxHash3::hash_code_combine_i32(1, 2);
    // Verify it produces consistent results
    assert_eq!(hash, XxHash3::hash_code_combine_i32(1, 2));
    // Different inputs should produce different hashes (usually)
    assert_ne!(hash, XxHash3::hash_code_combine_i32(1, 3));
}
