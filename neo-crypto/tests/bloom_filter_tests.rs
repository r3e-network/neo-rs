//! Comprehensive tests for BloomFilter

use neo_crypto::BloomFilter;

#[test]
fn test_bloom_filter_new() {
    let filter = BloomFilter::new(1024, 3, 123).unwrap();
    assert_eq!(filter.bit_size(), 1024);
    assert_eq!(filter.hash_functions(), 3);
    assert_eq!(filter.tweak(), 123);
}

#[test]
fn test_bloom_filter_zero_hash_functions() {
    let result = BloomFilter::new(1024, 0, 123);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("hash function count"));
}

#[test]
fn test_bloom_filter_zero_bit_size() {
    let result = BloomFilter::new(0, 3, 123);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("bit array size"));
}

#[test]
fn test_bloom_filter_add_and_check() {
    let mut filter = BloomFilter::new(1024, 3, 0).unwrap();

    let element = b"test element";
    filter.add(element);

    assert!(filter.check(element));
}

#[test]
fn test_bloom_filter_check_non_existent() {
    let mut filter = BloomFilter::new(1024, 3, 0).unwrap();

    filter.add(b"element1");

    // Element not added might return false (or true if hash collision)
    // We can't guarantee false negatives won't happen, but we test the behavior
    let _ = filter.check(b"element2");
}

#[test]
fn test_bloom_filter_multiple_elements() {
    let mut filter = BloomFilter::new(2048, 5, 42).unwrap();

    let elements = vec![
        b"element1".as_slice(),
        b"element2".as_slice(),
        b"element3".as_slice(),
        b"element4".as_slice(),
        b"element5".as_slice(),
    ];

    // Add all elements
    for element in &elements {
        filter.add(element);
    }

    // Check all elements are found
    for element in &elements {
        assert!(filter.check(element));
    }
}

#[test]
fn test_bloom_filter_with_bits() {
    let bits = vec![0xFF, 0x00, 0xAA, 0x55];
    let filter = BloomFilter::with_bits(32, 3, 123, &bits).unwrap();

    assert_eq!(filter.bit_size(), 32);
    assert_eq!(filter.hash_functions(), 3);
    assert_eq!(filter.tweak(), 123);

    let retrieved_bits = filter.bits();
    assert_eq!(retrieved_bits.len(), 4);
}

#[test]
fn test_bloom_filter_with_bits_truncates() {
    // Provide more bytes than needed
    let bits = vec![0xFF; 100];
    let filter = BloomFilter::with_bits(64, 2, 0, &bits).unwrap();

    assert_eq!(filter.bit_size(), 64);
    let retrieved = filter.bits();
    assert_eq!(retrieved.len(), 8); // 64 bits = 8 bytes
}

#[test]
fn test_bloom_filter_with_bits_pads() {
    // Provide fewer bytes than needed
    let bits = vec![0xFF, 0x00];
    let filter = BloomFilter::with_bits(64, 2, 0, &bits).unwrap();

    let retrieved = filter.bits();
    assert_eq!(retrieved.len(), 8); // Should be padded
    assert_eq!(retrieved[0], 0xFF);
    assert_eq!(retrieved[1], 0x00);
    assert_eq!(retrieved[2], 0x00); // Padded
}

#[test]
fn test_bloom_filter_empty_bytes() {
    let filter = BloomFilter::new(8, 1, 0).unwrap();
    let bits = filter.bits();
    assert_eq!(bits.len(), 1);
    assert_eq!(bits[0], 0);
}

#[test]
fn test_bloom_filter_different_tweaks() {
    let mut filter1 = BloomFilter::new(1024, 3, 100).unwrap();
    let mut filter2 = BloomFilter::new(1024, 3, 200).unwrap();

    let element = b"test";
    filter1.add(element);
    filter2.add(element);

    // Different tweaks should result in different bit patterns
    let bits1 = filter1.bits();
    let bits2 = filter2.bits();

    // They should both find the element
    assert!(filter1.check(element));
    assert!(filter2.check(element));

    // But bit patterns might differ (not guaranteed, but likely with good hash)
    // We just verify they work independently
}

#[test]
fn test_bloom_filter_hash_function_count() {
    let filter1 = BloomFilter::new(1024, 1, 0).unwrap();
    let filter2 = BloomFilter::new(1024, 10, 0).unwrap();

    assert_eq!(filter1.hash_functions(), 1);
    assert_eq!(filter2.hash_functions(), 10);
}

#[test]
fn test_bloom_filter_small_size() {
    let filter = BloomFilter::new(8, 1, 0).unwrap();
    assert_eq!(filter.bit_size(), 8);
}

#[test]
fn test_bloom_filter_large_size() {
    let filter = BloomFilter::new(1_000_000, 10, 0).unwrap();
    assert_eq!(filter.bit_size(), 1_000_000);
    assert_eq!(filter.hash_functions(), 10);
}

#[test]
fn test_bloom_filter_clone() {
    let mut filter = BloomFilter::new(1024, 3, 42).unwrap();
    filter.add(b"test");

    let cloned = filter.clone();
    assert_eq!(cloned.bit_size(), filter.bit_size());
    assert_eq!(cloned.hash_functions(), filter.hash_functions());
    assert_eq!(cloned.tweak(), filter.tweak());
    assert!(cloned.check(b"test"));
}

#[test]
fn test_bloom_filter_debug() {
    let filter = BloomFilter::new(1024, 3, 42).unwrap();
    let debug_str = format!("{:?}", filter);
    assert!(debug_str.contains("BloomFilter"));
}

#[test]
fn test_bloom_filter_bits_non_power_of_two() {
    let filter = BloomFilter::new(100, 3, 0).unwrap();
    assert_eq!(filter.bit_size(), 100);
    let bits = filter.bits();
    assert_eq!(bits.len(), 13); // ceil(100/8) = 13
}

#[test]
fn test_bloom_filter_consistent_checking() {
    let mut filter = BloomFilter::new(1024, 5, 123).unwrap();

    let element = b"consistent element";
    filter.add(element);

    // Multiple checks should return the same result
    for _ in 0..10 {
        assert!(filter.check(element));
    }
}

#[test]
fn test_bloom_filter_empty_element() {
    let mut filter = BloomFilter::new(1024, 3, 0).unwrap();

    filter.add(b"");
    assert!(filter.check(b""));
}

#[test]
fn test_bloom_filter_large_element() {
    let mut filter = BloomFilter::new(2048, 5, 0).unwrap();

    let large_element = vec![0xAB; 10000];
    filter.add(&large_element);
    assert!(filter.check(&large_element));
}

#[test]
fn test_bloom_filter_sequential_adds() {
    let mut filter = BloomFilter::new(4096, 7, 999).unwrap();

    for i in 0..100 {
        let element = format!("element_{}", i);
        filter.add(element.as_bytes());
    }

    // Verify all added elements
    for i in 0..100 {
        let element = format!("element_{}", i);
        assert!(filter.check(element.as_bytes()));
    }
}

#[test]
fn test_bloom_filter_binary_data() {
    let mut filter = BloomFilter::new(1024, 3, 0).unwrap();

    let binary_data = vec![0x00, 0xFF, 0xAA, 0x55, 0x12, 0x34, 0x56, 0x78];
    filter.add(&binary_data);
    assert!(filter.check(&binary_data));
}
