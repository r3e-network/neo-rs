use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::smart_contract::contract_state::NefFile;
use neo_core::smart_contract::method_token::MethodToken;
use neo_vm::ExecutionEngineLimits;

const NEF_MAGIC: u32 = 0x3346_454E;
const COMPILER_LEN: usize = 64;

fn sample_nef() -> NefFile {
    let mut file = NefFile {
        compiler: " ".repeat(32),
        source: String::new(),
        tokens: Vec::new(),
        script: vec![0x01, 0x02, 0x03],
        checksum: 0,
    };
    file.update_checksum();
    file
}

fn serialize_nef(file: &NefFile) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    file.serialize(&mut writer).expect("serialize nef");
    writer.into_bytes()
}

fn build_nef_bytes(script: &[u8], checksum: u32) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(NEF_MAGIC).expect("magic");

    let compiler = " ".repeat(32);
    let mut fixed = [0u8; COMPILER_LEN];
    let compiler_bytes = compiler.as_bytes();
    fixed[..compiler_bytes.len()].copy_from_slice(compiler_bytes);
    writer.write_bytes(&fixed).expect("compiler");

    writer.write_var_string("").expect("source");
    writer.write_u8(0).expect("reserved");

    let tokens: Vec<MethodToken> = Vec::new();
    writer.write_serializable_vec(&tokens).expect("tokens");

    writer.write_u16(0).expect("reserved2");
    writer.write_var_bytes(script).expect("script");
    writer.write_u32(checksum).expect("checksum");

    writer.into_bytes()
}

#[test]
fn nef_roundtrip_matches_csharp() {
    let file = sample_nef();
    let bytes = serialize_nef(&file);
    let parsed = NefFile::parse(&bytes).expect("parse nef");

    assert_eq!(parsed.compiler, file.compiler);
    assert_eq!(parsed.checksum, file.checksum);
    assert_eq!(parsed.script, file.script);
}

#[test]
fn nef_size_matches_csharp() {
    let file = sample_nef();
    let expected = 4 + 64 + 1 + 1 + 1 + 2 + 4 + 4;
    assert_eq!(file.size(), expected);
}

#[test]
fn nef_deserialize_rejects_wrong_magic() {
    let file = sample_nef();
    let mut bytes = serialize_nef(&file);
    bytes[..4].copy_from_slice(&0u32.to_le_bytes());

    let mut reader = MemoryReader::new(&bytes);
    assert!(NefFile::deserialize(&mut reader).is_err());
}

#[test]
fn nef_deserialize_rejects_bad_checksum() {
    let mut file = sample_nef();
    file.checksum = file.checksum.wrapping_add(1);
    let bytes = serialize_nef(&file);

    let mut reader = MemoryReader::new(&bytes);
    assert!(NefFile::deserialize(&mut reader).is_err());
}

#[test]
fn nef_deserialize_rejects_empty_script() {
    let bytes = build_nef_bytes(&[], 0);
    let mut reader = MemoryReader::new(&bytes);
    assert!(NefFile::deserialize(&mut reader).is_err());
}

#[test]
fn nef_serialize_rejects_overlong_compiler() {
    let mut file = sample_nef();
    file.compiler = "a".repeat(COMPILER_LEN + 1);
    let mut writer = BinaryWriter::new();
    assert!(file.serialize(&mut writer).is_err());
}

#[test]
fn nef_deserialize_rejects_oversize_script() {
    let max = ExecutionEngineLimits::default().max_item_size as usize;
    let bytes = build_nef_bytes(&vec![0u8; max + 1], 0);
    let mut reader = MemoryReader::new(&bytes);
    assert!(NefFile::deserialize(&mut reader).is_err());
}

#[test]
fn nef_serialize_rejects_oversize_script() {
    let max = ExecutionEngineLimits::default().max_item_size as usize;
    let mut file = sample_nef();
    file.script = vec![0u8; max + 1];
    let mut writer = BinaryWriter::new();
    assert!(file.serialize(&mut writer).is_err());
}
