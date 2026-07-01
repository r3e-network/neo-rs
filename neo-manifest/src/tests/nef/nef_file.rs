use super::*;

#[test]
fn magic_constant_matches_neo_spec() {
    // 0x3346454E = 'N','E','F',3 in little-endian
    assert_eq!(NefFile::MAGIC, 0x3346_454E);
}

#[test]
fn new_computes_checksum() {
    let nef = NefFile::new("neo-core-v0.0.0".to_string(), vec![0x40]); // RET
    assert_ne!(nef.checksum, 0);
}

#[test]
fn default_has_zero_checksum() {
    let nef = NefFile::default();
    assert_eq!(nef.checksum, 0);
}

#[test]
fn new_constructor_stores_fields() {
    let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3, 4]);
    assert_eq!(nef.compiler, "compiler");
    assert_eq!(nef.script, vec![1, 2, 3, 4]);
    assert!(nef.tokens.is_empty());
    assert!(nef.source.is_empty());
}

#[test]
fn size_includes_all_fields() {
    let nef = NefFile::new("c".to_string(), vec![0; 100]);
    // 4 (magic) + 64 (compiler fixed) + 1 (source var int) + 1 (reserved)
    // + 1 (tokens var int) + 2 (reserved u16) + 2 (script var int) + 4 (checksum)
    // + the actual bytes
    let size = nef.size();
    assert!(size > 4 + 64 + 1 + 1 + 1 + 2 + 2 + 4);
}

#[test]
fn from_bytes_rejects_bad_magic() {
    let bytes = vec![0xFF; 100];
    let result = NefFile::from_bytes(&bytes);
    assert!(result.is_err());
}
