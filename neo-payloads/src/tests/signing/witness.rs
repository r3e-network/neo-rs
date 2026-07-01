use super::*;
use neo_io::Serializable;

#[test]
fn test_witness_new() {
    let witness = Witness::new();
    assert!(witness.invocation_script.is_empty());
    assert!(witness.verification_script.is_empty());
    assert!(witness.script_hash.get().is_none());
}

#[test]
fn test_witness_empty() {
    let witness = Witness::empty();
    assert!(witness.invocation_script.is_empty());
    assert!(witness.verification_script.is_empty());
}

#[test]
fn test_witness_new_with_scripts() {
    let invocation = vec![1, 2, 3];
    let verification = vec![4, 5, 6];
    let witness = Witness::new_with_scripts(invocation.clone(), verification.clone());
    assert_eq!(witness.invocation_script, invocation);
    assert_eq!(witness.verification_script, verification);
}

#[test]
fn test_witness_size() {
    let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    let size = witness.size();
    assert_eq!(size, 8);
}

#[test]
fn witness_size_matches_serialized_length_at_var_size_boundaries() {
    for len in [0, 1, 252, 253, 254, 1024] {
        let invocation = vec![0xAA; len];
        let verification = vec![0xBB; len];
        let witness = Witness::new_with_scripts(invocation, verification);
        let mut writer = neo_io::BinaryWriter::new();

        <Witness as Serializable>::serialize(&witness, &mut writer).unwrap();

        assert_eq!(witness.size(), writer.as_bytes().len());
        assert_eq!(
            witness.size(),
            SerializeHelper::get_var_size_bytes(&witness.invocation_script)
                + SerializeHelper::get_var_size_bytes(&witness.verification_script)
        );
    }
}

#[test]
fn test_witness_clone() {
    let original = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    let cloned = original.clone_witness();
    assert_eq!(original.invocation_script, cloned.invocation_script);
    assert_eq!(original.verification_script, cloned.verification_script);
}

#[test]
fn test_witness_serialization() {
    let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
    let mut writer = neo_io::BinaryWriter::new();
    <Witness as Serializable>::serialize(&witness, &mut writer).unwrap();
    let bytes = writer.to_bytes();
    let mut reader = neo_io::MemoryReader::new(&bytes);
    let deserialized = <Witness as Serializable>::deserialize(&mut reader).unwrap();
    assert_eq!(witness.invocation_script, deserialized.invocation_script);
    assert_eq!(
        witness.verification_script,
        deserialized.verification_script
    );
}
