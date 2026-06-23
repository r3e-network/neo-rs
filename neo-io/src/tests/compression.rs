use super::*;

#[test]
fn compress_decompress_roundtrip() {
    let data = b"Hello, Neo blockchain! This is a test of LZ4 compression.";
    let compressed = Lz4::compress_lz4(data).unwrap();
    let decompressed = Lz4::decompress_lz4(&compressed, 1024).unwrap();
    assert_eq!(data.as_slice(), decompressed.as_slice());
}

#[test]
fn decompress_rejects_declared_size_over_limit_before_decode() {
    let mut compressed = Vec::new();
    compressed.extend_from_slice(&1024u32.to_le_bytes());
    compressed.extend_from_slice(&[0u8; 8]);

    match Lz4::decompress_lz4(&compressed, 16) {
        Err(e) => assert!(e.to_string().contains("exceeds maximum size")),
        other => panic!("expected compression error, got {other:?}"),
    }
}

#[test]
fn decompress_rejects_short_data() {
    match Lz4::decompress_lz4(&[0u8; 2], 1024) {
        Err(e) => assert!(e.to_string().contains("missing length prefix")),
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn decompress_does_not_panic_on_malformed_block() {
    // Regression for a fuzzer-found DoS (fuzz_message_parse, crash
    // //8IAAAAAAAAAPs=): a crafted compressed P2P payload made lz4_flex 0.10.0
    // panic inside its decode sink instead of returning an error. With
    // panic = "abort" in release, that crashes the whole node — peer-reachable
    // via the compressed-message path. The decompressor must reject malformed
    // input gracefully. This payload declares size 0 but carries trailing block
    // bytes the decoder would otherwise try to emit.
    let malformed = [0u8, 0, 0, 0, 0, 0, 0, 0xFB];
    assert!(
        Lz4::decompress_lz4(&malformed, 0x0200_0000).is_err(),
        "malformed LZ4 block must return Err, not panic"
    );
}
