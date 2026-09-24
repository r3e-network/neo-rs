// Neo N3 VarInt Encoding Refinement Tests
// Test suite for var_int.rs (lines 1-84) verification against Coq model

use neo_io::var_int::{encoded_len, read_var_int_prefix, write_var_int};

/// Suite 1: Single Byte Range (0-252) - exhaustive
#[test]
fn test_single_byte_range_0_to_252_exhaustive() {
    // All values 0-252 encode as single bytes per encoded_len() line 70
    for n in 0..=252u64 {
        let mut buf = Vec::with_capacity(1);
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 1, "Value {} should be single byte", n);
        assert_eq!(buf[0], n as u8, "Byte value mismatch for {}", n);
        
        let result = read_var_int_prefix(&buf);
        assert!(result.is_some(), "Failed to decode {}", n);
        
        let (decoded, width) = result.unwrap();
        assert_eq!(decoded, n, "Round-trip failed for {}", n);
        assert_eq!(width, 1, "Width should be 1 for {}", n);
    }
    
    eprintln!("✓ Exhausted single-byte range 0-252 ({} encodings)", 253u64);
}

/// Suite 2: Three Byte Range (253-65535) - u16 marker
#[test]
fn test_three_byte_range_253_to_65535() {
    let test_values = vec![253, 254, 500, 1000, 10000, 65534, 65535];
    
    for n in test_values {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 3, "Value {} should be 3 bytes", n);
        assert_eq!(buf[0], 0xFD, "Marker should be FD for {}", n);
        
        let (decoded, width) = read_var_int_prefix(&buf).unwrap();
        assert_eq!(decoded, n, "Decode mismatch for {}", n);
        assert_eq!(width, 3, "Width should be 3 for {}", n);
        
        eprintln!("  ✓ {} → [FD, {}, {}]", n, buf[1], buf[2]);
    }
}

/// Suite 3: Five Byte Range (65536-4294967295) - u32 marker  
#[test]
fn test_five_byte_range_65536_to_u32_max() {
    let test_values = vec![
        65536,           // First 5-byte encoding
        100_000,         // ~100k
        1_000_000,       // ~1M
        10_000_000,      // ~10M
        1_000_000_000,   // ~1B
        4_294_967_295,   // Max u32
    ];
    
    for n in test_values {
        let mut buf = Vec::with_capacity(5);
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 5, "Value {} should be 5 bytes", n);
        assert_eq!(buf[0], 0xFE, "Marker should be FE for {}", n);
        
        let (decoded, width) = read_var_int_prefix(&buf).unwrap();
        assert_eq!(decoded, n, "Decode mismatch for {}", n);
        assert_eq!(width, 5, "Width should be 5 for {}", n);
        
        // Verify little-endian reconstruction
        let reconstructed = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]) as u64;
        assert_eq!(reconstructed, n, "LE reconstruction failed for {}", n);
        
        eprintln!("  ✓ {} → [FE, {:?} (LE)]", n, &buf[1..5]);
    }
}

/// Suite 4: Nine Byte Range (4294967296+) - u64 marker
#[test]
fn test_nine_byte_range_u32_max_plus() {
    let test_values = vec![
        4_294_967_296,   // First 9-byte (u32::MAX + 1)
        10_000_000_000,  // 10 billion
        100_000_000_000, // 100 billion  
        1_000_000_000_000, // 1 trillion
        u64::MAX / 2,    // Half of max u64
        u64::MAX,        // Absolute maximum
    ];
    
    for n in test_values {
        let mut buf = Vec::with_capacity(9);
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 9, "Value {} should be 9 bytes", n);
        assert_eq!(buf[0], 0xFF, "Marker should be FF for {}", n);
        
        let (decoded, width) = read_var_int_prefix(&buf).unwrap();
        assert_eq!(decoded, n, "Decode mismatch for {}", n);
        assert_eq!(width, 9, "Width should be 9 for {}", n);
        
        // Verify full u64 LE reconstruction
        let reconstructed = u64::from_le_bytes([
            buf[1], buf[2], buf[3], buf[4], 
            buf[5], buf[6], buf[7], buf[8]
        ]);
        assert_eq!(reconstructed, n, "Full u64 LE reconstruction failed for {}", n);
        
        eprintln!("  ✓ {} → [FF, ...] (verified)", n);
    }
}

/// Critical Boundary: First 3-byte at exactly 253
#[test]
fn test_exact_boundary_253_first_three_byte() {
    let mut buf = Vec::new();
    write_var_int(253, &mut buf);
    
    // Expected: [253, 253, 0] per existing test at line 141
    assert_eq!(buf, vec![253, 253, 0], "Exact byte sequence for 253 must match");
    
    let (decoded, width) = read_var_int_prefix(&buf).unwrap();
    assert_eq!(decoded, 253);
    assert_eq!(width, 3);
    
    assert_eq!(encoded_len(253), 3);
    
    eprintln!("✓ Exact boundary: 253 is first 3-byte encoding");
}

/// Critical Boundary: First 5-byte at exactly 65536
#[test]
fn test_exact_boundary_65536_first_five_byte() {
    let mut buf = Vec::new();
    write_var_int(65536, &mut buf);
    
    // Expected: [254, 0, 0, 1, 0] per line 143
    assert_eq!(buf, vec![254, 0, 0, 1, 0], "Exact byte sequence for 65536");
    
    let (decoded, width) = read_var_int_prefix(&buf).unwrap();
    assert_eq!(decoded, 65536);
    assert_eq!(width, 5);
    
    assert_eq!(encoded_len(65536), 5);
    
    eprintln!("✓ Exact boundary: 65536 is first 5-byte encoding");
}

/// Critical Boundary: Maximum 3-byte at 65535
#[test]
fn test_maximum_value_stays_in_three_byte() {
    let mut buf = Vec::new();
    write_var_int(65535, &mut buf);
    
    // Expected: [253, 255, 255] per line 142
    assert_eq!(buf, vec![253, 255, 255], "65535 stays 3 bytes");
    
    let decoded = read_var_int_prefix(&buf).map(|(v, _)| v).unwrap();
    assert_eq!(decoded, 65535);
    
    assert_eq!(encoded_len(65535), 3);
    
    // Verify next value requires more bytes
    assert_eq!(encoded_len(65536), 5, "Next value jumps to 5 bytes");
    
    eprintln!("✓ Maximum 3-byte value: 65535 encoded compactly");
}

/// Critical Boundary: Maximum 5-byte at 4294967295
#[test]
fn test_maximum_value_stays_in_five_byte() {
    let u32_max = 4_294_967_295u64;
    
    let mut buf = Vec::new();
    write_var_int(u32_max, &mut buf);
    
    // Expected: [254, 255, 255, 255, 255] per line 144
    assert_eq!(buf, vec![254, 255, 255, 255, 255]);
    
    let decoded = read_var_int_prefix(&buf).map(|(v, _)| v).unwrap();
    assert_eq!(decoded, u32_max);
    
    assert_eq!(encoded_len(u32_max), 5);
    
    // Verify next value requires more bytes
    assert_eq!(encoded_len(u32_max + 1), 9, "u32::MAX + 1 requires 9 bytes");
    
    eprintln!("✓ Maximum 5-byte value: {} encoded compactly", u32_max);
}

/// Known Sequences Verification Against Existing Unit Tests
#[test]
fn test_known_sequences_match_rfc_varint_examples() {
    // Example 1: 252 encodes as [252] (single byte, per test line 140)
    let mut buf = Vec::new();
    write_var_int(252, &mut buf);
    assert_eq!(buf, vec![252]);
    
    // Example 2: 253 encodes as [253, 253, 0] (3-byte, per test line 141)
    buf.clear();
    write_var_int(253, &mut buf);
    assert_eq!(buf, vec![253, 253, 0]);
    
    // Example 3: 65535 encodes as [253, 255, 255] (max 3-byte, per test line 142)
    buf.clear();
    write_var_int(65535, &mut buf);
    assert_eq!(buf, vec![253, 255, 255]);
    
    // Example 4: 65536 encodes as [254, 0, 0, 1, 0] (first 5-byte, per test line 143)
    buf.clear();
    write_var_int(65536, &mut buf);
    assert_eq!(buf, vec![254, 0, 0, 1, 0]);
    
    // Example 5: 4294967295 encodes as [254, 255, 255, 255, 255] (per test line 144)
    buf.clear();
    write_var_int(4_294_967_295, &mut buf);
    assert_eq!(buf, vec![254, 255, 255, 255, 255]);
    
    // Example 6: 4294967296 encodes as [255, 0, 0, 0, 0, 1, 0, 0, 0] (per test line 145)
    buf.clear();
    write_var_int(4_294_967_296, &mut buf);
    assert_eq!(buf, vec![255, 0, 0, 0, 0, 1, 0, 0, 0]);
    
    eprintln!("✓ All 6 RFC-like sequences verified against canonical examples");
}

/// Partial/Incomplete Buffer Handling
#[test]
fn test_partial_incomplete_encodings_return_none() {
    // Incomplete 3-byte encoding
    assert_eq!(read_var_int_prefix(&[0xFD]), None);
    assert_eq!(read_var_int_prefix(&[0xFD, 0x00]), None);
    
    // Incomplete 5-byte encoding  
    assert_eq!(read_var_int_prefix(&[0xFE]), None);
    assert_eq!(read_var_int_prefix(&[0xFE, 0x00]), None);
    assert_eq!(read_var_int_prefix(&[0xFE, 0x00, 0x00]), None);
    assert_eq!(read_var_int_prefix(&[0xFE, 0x00, 0x00, 0x00]), None);
    
    // Incomplete 9-byte encoding
    assert_eq!(read_var_int_prefix(&[0xFF]), None);
    assert_eq!(read_var_int_prefix(&[0xFF, 0x00]), None);
    assert_eq!(read_var_int_prefix(&[0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]), None);
    
    eprintln!("✓ All incomplete buffer scenarios return None safely");
}

/// Empty Buffer Edge Case
#[test]
fn test_empty_buffer_returns_none() {
    // Line 17: prefix = *src.first()? returns None for empty slice
    assert_eq!(read_var_int_prefix(&[]), None);
    
    eprintln!("✓ Empty buffer gracefully handled");
}

/// Length Calculation Accuracy
#[test]
fn test_length_calculations_match_definitions() {
    let test_cases = vec![
        (0, 1),
        (127, 1),
        (128, 1),
        (252, 1),
        (253, 3),
        (1000, 3),
        (65535, 3),
        (65536, 5),
        (1_000_000, 5),
        (4_294_967_295, 5),
        (4_294_967_296, 9),
        (u64::MAX, 9),
    ];
    
    for (value, expected_len) in &test_cases {
        let actual_len = encoded_len(*value);
        assert_eq!(actual_len, *expected_len, "Length calculation failed for {}", value);
        
        let mut buf = Vec::new();
        write_var_int(*value, &mut buf);
        assert_eq!(buf.len(), *expected_len, "Actual encoding size mismatch for {}", value);
        
        let (_, width) = read_var_int_prefix(&buf).unwrap();
        assert_eq!(width, *expected_len);
    }
    
    eprintln!("✓ All {} length calculations verified against encodings", test_cases.len());
}

/// Round-trip Integrity Across All Ranges
#[test]
fn test_comprehensive_roundtrip_all_ranges() {
    let mut total_tests = 0;
    
    // Region 1: [0, 252] - sample every 5th value
    for n in (0..=252).step_by(5) {
        roundtrip_check(n, &mut total_tests);
    }
    
    // Region 2: [253, 65535] - sample powers of 2
    for exp in 8..16 {
        let base = 1 << exp;
        roundtrip_check(base - 1, &mut total_tests);
        roundtrip_check(base, &mut total_tests);      
        roundtrip_check(base + 1, &mut total_tests);
    }
    
    // Region 3: [65536, 4294967295] - sample powers of 2
    for exp in 17..=32 {
        roundtrip_check(1 << exp, &mut total_tests);
        roundtrip_check((1 << exp) - 1, &mut total_tests);
    }
    
    // Region 4: [4294967296+, u64::MAX]
    roundtrip_check(2u64.pow(33), &mut total_tests);
    roundtrip_check(2u64.pow(40), &mut total_tests);
    roundtrip_check(2u64.pow(50), &mut total_tests);
    roundtrip_check(2u64.pow(63), &mut total_tests);
    
    eprintln!("✓ Comprehensive roundtrip verified across {} test points", total_tests);
}

/// Helper for roundtrip testing
fn roundtrip_check(n: u64, counter: &mut usize) {
    let mut buf = Vec::new();
    write_var_int(n, &mut buf);
    
    let result = read_var_int_prefix(&buf);
    assert!(result.is_some(), "Failed for {}", n);
    
    let (decoded, width) = result.unwrap();
    assert_eq!(decoded, n, "Roundtrip failed for {}", n);
    assert_eq!(width, buf.len());
    
    *counter += 1;
}

/// Verify Little-Endian Byte Ordering
#[test]
fn test_little_endian_byte_ordering() {
    // Value 0x0001_0000 = 65536 = [0xFE, 0x00, 0x00, 0x01, 0x00]
    let mut buf = Vec::new();
    write_var_int(65536, &mut buf);
    assert_eq!(buf, vec![254, 0, 0, 1, 0]);
    
    // Extract bytes individually and verify positions
    let byte0 = buf[1] as u64;
    let byte1 = buf[2] as u64;
    let byte2 = buf[3] as u64;
    let byte3 = buf[4] as u64;
    
    let reconstructed = byte0 + 256*byte1 + 65536*byte2 + 16777216*byte3;
    assert_eq!(reconstructed, 65536);
    
    // Same test for u64 example at line 145: 0x0001_0000_0000
    buf.clear();
    write_var_int(4_294_967_296, &mut buf);
    assert_eq!(buf, vec![255, 0, 0, 0, 0, 1, 0, 0, 0]);
    
    let reconstructed = u64::from_le_bytes([
        buf[1], buf[2], buf[3], buf[4],
        buf[5], buf[6], buf[7], buf[8]
    ]);
    assert_eq!(reconstructed, 4_294_967_296);
    
    eprintln!("✓ Explicit little-endian ordering verified");
}
