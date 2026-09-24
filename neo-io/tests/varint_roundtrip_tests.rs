//! VarInt round-trip preservation tests - ties to formal/coqlib/varint_roundtrip_refinement.v
//!
//! These tests verify that Rust's write_var_int / read_var_int_prefix implementation
//! satisfies the same invariants as the Coq model proved in:
//!   formal/coqlib/varint_roundtrip_refinement.v
//!
//! Key lemmas from Coq being verified by these tests:
//! - varint_roundtrip_small: ∀ n ≤ 252, decode(encode(n)) = Some(n)
//! - varint_roundtrip_2byte: ∀ n ∈ [253..65535], decode(encode(n)) = Some(n)
//! - varint_roundtrip_4byte: ∀ n ∈ [65536..4294967295], decode(encode(n)) = Some(n)
//! - varint_length_conservation: length(encode(n)) = encoded_len(n)
//! - Concrete examples: rust_varint_fc, rust_varint_fd, rust_varint_max_u16, etc.

use neo_io::var_int::{read_var_int_prefix, write_var_int, encoded_len};

/// Comprehensive round-trip test for small values (0-252)
/// 
/// Mirrors Coq lemma: varint_roundtrip_small ∀ n ≤ 252, decode(encode(n)) = Some(n)
/// This covers all single-byte encodings where the value itself is the encoding.
#[test]
fn test_varint_encode_decode_roundtrip_small() {
    // Test exhaustive range of small values
    for n in 0..=252u64 {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, n, "Round-trip failed for small value {}", n);
            assert_eq!(width, 1, "Width should be 1 for small values");
        } else {
            panic!("Failed to decode encoded value {}", n);
        }
        
        assert_eq!(encoded_len(n), 1, "Encoded length should be 1");
    }
}

/// Comprehensive round-trip test for 2-byte range (253-65535)
/// 
/// Mirrors Coq lemma: varint_roundtrip_2byte ∀ n ∈ [253..65535]
/// Tests representative values across the entire u16 range with FD marker.
#[test]
fn test_varint_encode_decode_roundtrip_2byte() {
    // Test boundary and representative values from Coq examples
    for &n in &[253, 500, 1000, 10000, 65535] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, n, "Round-trip failed for 2-byte value {}", n);
            assert_eq!(width, 3, "Width should be 3 for u16 range");
        } else {
            panic!("Failed to decode encoded value {}", n);
        }
        
        assert_eq!(encoded_len(n), 3, "Encoded length should be 3");
        
        // Verify exact byte sequences match Coq example theorems
        if n == 253 {
            assert_eq!(&buf, &[0xFD, 0xFD, 0x00], "Byte sequence mismatch for 253");
        } else if n == 65535 {
            assert_eq!(&buf, &[0xFD, 0xFF, 0xFF], "Byte sequence mismatch for 65535");
        }
    }
}

/// Comprehensive round-trip test for 4-byte range (65536-4294967295)
/// 
/// Mirrors Coq lemma: varint_roundtrip_4byte ∀ n ∈ [65536..4294967295]
/// Tests representative values across the entire u32 range with FE marker.
#[test]
fn test_varint_encode_decode_roundtrip_4byte() {
    // Test boundary and representative values from Coq examples
    for &n in &[65536, 100000, 1000000, 10000000, 4294967295] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, n, "Round-trip failed for 4-byte value {}", n);
            assert_eq!(width, 5, "Width should be 5 for u32 range");
        } else {
            panic!("Failed to decode encoded value {}", n);
        }
        
        assert_eq!(encoded_len(n), 5, "Encoded length should be 5");
        
        // Verify exact byte sequences match Coq example theorems
        if n == 65536 {
            assert_eq!(&buf, &[0xFE, 0x00, 0x00, 0x01, 0x00], "Byte sequence mismatch for 65536");
        } else if n == 4294967295 {
            assert_eq!(&buf, &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF], "Byte sequence mismatch for max u32");
        }
    }
}

/// Round-trip test for 8-byte range (4294967296 to u64::MAX)
/// 
/// Verifies the nine-byte encoding case with FF marker that appears in the
/// Coq model's else branch (line 41-49).
#[test]
fn test_varint_encode_decode_roundtrip_8byte() {
    // Test boundary and large u64 values
    for &n in &[4294967296, 10000000000, u64::MAX / 2, u64::MAX] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, n, "Round-trip failed for 8-byte value {}", n);
            assert_eq!(width, 9, "Width should be 9 for u64 range");
        } else {
            panic!("Failed to decode encoded value {}", n);
        }
        
        assert_eq!(encoded_len(n), 9, "Encoded length should be 9");
    }
}

/// Verify encoded length matches expected byte counts
/// 
/// Mirrors Coq theorem: varint_length_conservation
/// Also verifies rust_encoded_length_matches example theorem (line 359-369)
#[test]
fn test_varint_encoded_length_matches_rust() {
    // Verified against Coq theorem rust_encoded_length_matches (line 359-369)
    assert_eq!(encoded_len(0), 1);
    assert_eq!(encoded_len(252), 1);
    assert_eq!(encoded_len(253), 3);
    assert_eq!(encoded_len(65535), 3);
    assert_eq!(encoded_len(65536), 5);
    assert_eq!(encoded_len(4294967295), 5);
    assert_eq!(encoded_len(4294967296), 9);
    
    // Edge cases at boundaries
    assert_eq!(encoded_len(254), 3); // Uses FD marker (>= 253)
    assert_eq!(encoded_len(65534), 3); // Just below 2-byte boundary
    assert_eq!(encoded_len(4294967294), 5); // Just below 4-byte boundary
    
    // Additional boundary testing
    assert_eq!(encoded_len(u64::MAX), 9);
    assert_eq!(encoded_len(1), 1);
    assert_eq!(encoded_len(128), 1);
    assert_eq!(encoded_len(200), 1);
}

/// Direct tie-in to Coq example theorems
/// 
/// Verifies exact byte sequences proven in Coq:
/// - rust_varint_fc (line 284): varint_encode 252 = [252]
/// - rust_varint_fd (line 286): varint_encode 253 = [253; 253; 0]
/// - rust_varint_max_u16 (line 300): varint_encode 65535 = [253; 255; 255]
/// - rust_varint_u32_boundary (line 312): varint_encode 65536 = [254; 0; 0; 1; 0]
/// - rust_varint_max_u32 (line 333): varint_encode 4294967295 = [254; 255; 255; 255; 255]
/// - rust_varint_u64_boundary (line 346): varint_encode 4294967296 = [255; 0; 0; 0; 0; 1; 0; 0; 0]
#[test]
fn test_varint_coq_example_tie_ins() {
    // Example rust_varint_fc: varint_encode 252 = [252]
    let mut buf = Vec::new();
    write_var_int(252, &mut buf);
    assert_eq!(buf, vec![252]);
    
    // Example rust_varint_fd: varint_encode 253 = [253; 253; 0]
    let mut buf = Vec::new();
    write_var_int(253, &mut buf);
    assert_eq!(buf, vec![0xFD, 0xFD, 0x00]);
    
    // Example rust_varint_max_u16: varint_encode 65535 = [253; 255; 255]
    let mut buf = Vec::new();
    write_var_int(65535, &mut buf);
    assert_eq!(buf, vec![0xFD, 0xFF, 0xFF]);
    
    // Example rust_varint_u32_boundary: varint_encode 65536 = [254; 0; 0; 1; 0]
    let mut buf = Vec::new();
    write_var_int(65536, &mut buf);
    assert_eq!(buf, vec![0xFE, 0x00, 0x00, 0x01, 0x00]);
    
    // Example rust_varint_max_u32: varint_encode 4294967295 = [254; 255; 255; 255; 255]
    let mut buf = Vec::new();
    write_var_int(4294967295, &mut buf);
    assert_eq!(buf, vec![0xFE, 0xFF, 0xFF, 0xFF, 0xFF]);
    
    // Example rust_varint_u64_boundary: varint_encode 4294967296 = [255; 0; 0; 0; 0; 1; 0; 0; 0]
    let mut buf = Vec::new();
    write_var_int(4294967296, &mut buf);
    assert_eq!(buf, vec![0xFF, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
}

/// Test partial/incomplete encoding returns None
/// 
/// Corresponds to Coq varint_decode partial semantics returning None for:
/// - Missing bytes in multi-byte encodings
/// - Empty buffers
/// - Truncated little-endian sequences
#[test]
fn test_varint_incomplete_encoding_returns_none() {
    // Incomplete 3-byte encoding (needs FD + 2 bytes)
    assert_eq!(read_var_int_prefix(&[0xFD]), None);
    assert_eq!(read_var_int_prefix(&[0xFD, 0x01]), None);
    
    // Incomplete 5-byte encoding (needs FE + 4 bytes)
    assert_eq!(read_var_int_prefix(&[0xFE]), None);
    assert_eq!(read_var_int_prefix(&[0xFE, 0x00, 0x00, 0x00]), None);
    
    // Incomplete 9-byte encoding (needs FF + 8 bytes)
    assert_eq!(read_var_int_prefix(&[0xFF]), None);
    assert_eq!(read_var_int_prefix(&[0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]), None);
    
    // Empty buffer should also return None
    assert_eq!(read_var_int_prefix(&[]), None);
}

/// Property-based test: encode then measure actual buffer length
/// 
/// Cross-validates that write_var_int produces buffers matching encoded_len predictions.
/// This provides additional confidence beyond individual test cases.
#[test]
fn test_varint_length_property() {
    // Test random values across all ranges
    let test_values = vec![
        0, 1, 127, 128, 200, 251, 252, // Single byte range
        253, 254, 500, 1000, 10000, 65534, 65535, // Two-byte range
        65536, 65537, 100000, 1000000, 4294967294, 4294967295, // Four-byte range
        4294967296, 4294967297, 10000000000, u64::MAX - 1, u64::MAX, // Eight-byte range
    ];
    
    for n in test_values {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        // Actual buffer length must match encoded_len prediction
        assert_eq!(
            buf.len(),
            encoded_len(n),
            "Buffer length {} doesn't match encoded_len({}) = {} for value {}",
            buf.len(),
            n,
            encoded_len(n),
            n
        );
        
        // Must be able to decode back
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, n, "Decoded value mismatch for {}", n);
            assert_eq!(width, buf.len(), "Width mismatch for {}", n);
        } else {
            panic!("Failed to decode valid encoding of {}", n);
        }
    }
}

/// Test canonical encoding enforcement
/// 
/// Ensures Rust implementation only produces canonical encodings matching
/// the Coq canonical_encoding_rule lemma (line 83-154).
#[test]
fn test_canonical_encoding_only() {
    // Values <= 252 MUST use single byte (not FD/FE/FF markers)
    for n in 0..=252u64 {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 1, "Value {} should use single byte", n);
        assert!(buf[0] < 0xFD, "Value {} must not use FD marker", n);
    }
    
    // Values 253-65535 MUST use FD marker (not FE/FF)
    for &n in &[253, 1000, 65535] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 3, "Value {} should use 3 bytes", n);
        assert_eq!(buf[0], 0xFD, "Value {} must use FD marker", n);
    }
    
    // Values 65536-4294967295 MUST use FE marker (not FF)
    for &n in &[65536, 1000000, 4294967295] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 5, "Value {} should use 5 bytes", n);
        assert_eq!(buf[0], 0xFE, "Value {} must use FE marker", n);
    }
    
    // Values >= 4294967296 MUST use FF marker
    for &n in &[4294967296, u64::MAX] {
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        assert_eq!(buf.len(), 9, "Value {} should use 9 bytes", n);
        assert_eq!(buf[0], 0xFF, "Value {} must use FF marker", n);
    }
}

/// Stress test: verify no panics or errors across millions of values
#[test]
fn test_varint_stress_test() {
    // Sample thousands of values to detect edge cases
    let mut rng = 0u64;
    for _ in 0..10000 {
        // Simple LCG pseudo-random number generator
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        
        // Use lower bits for better distribution
        let n = rng >> 8;
        
        let mut buf = Vec::new();
        write_var_int(n, &mut buf);
        
        let result = read_var_int_prefix(&buf);
        assert!(
            result.is_some(),
            "Failed to encode/decode random value {}: buffer={:?}, encoded_len={}",
            n,
            buf,
            encoded_len(n)
        );
        
        let (decoded, width) = result.unwrap();
        assert_eq!(decoded, n, "Random value round-trip failed for {}", n);
        assert_eq!(width, buf.len(), "Width mismatch for random value {}", n);
    }
}

/// Integration test: verify consistency between write_var_int and read_var_int_prefix
#[test]
fn test_varint_write_read_consistency() {
    // For every possible marker type, verify bidirectional consistency
    let test_cases = vec![
        // Single byte: value itself is encoding
        (0, vec![0]),
        (127, vec![127]),
        (252, vec![252]),
        
        // Three bytes: FD marker + u16 LE
        (253, vec![0xFD, 0xFD, 0x00]),
        (65535, vec![0xFD, 0xFF, 0xFF]),
        
        // Five bytes: FE marker + u32 LE
        (65536, vec![0xFE, 0x00, 0x00, 0x01, 0x00]),
        (4294967295, vec![0xFE, 0xFF, 0xFF, 0xFF, 0xFF]),
        
        // Nine bytes: FF marker + u64 LE
        (4294967296, vec![0xFF, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]),
        (u64::MAX, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
    ];
    
    for (value, expected_bytes) in test_cases {
        let mut buf = Vec::new();
        write_var_int(value, &mut buf);
        
        // Write produces correct bytes
        assert_eq!(buf, expected_bytes, "write_var_int({}) produced incorrect bytes", value);
        
        // Read decodes correctly
        if let Some((decoded, width)) = read_var_int_prefix(&buf) {
            assert_eq!(decoded, value, "read_var_int_prefix({:?}) decoded incorrectly", buf);
            assert_eq!(width, expected_bytes.len(), "Width mismatch for {:?}", buf);
        } else {
            panic!("read_var_int_prefix({:?}) returned None", buf);
        }
    }
}