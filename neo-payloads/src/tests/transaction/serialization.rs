use super::*;
use neo_primitives::WitnessScope;

fn write_unsigned_header(writer: &mut BinaryWriter) {
    writer.write_u8(0).unwrap(); // Version
    writer.write_u32(0x0102_0304).unwrap(); // Nonce
    writer.write_i64(0).unwrap(); // SystemFee
    writer.write_i64(0).unwrap(); // NetworkFee
    writer.write_u32(42).unwrap(); // ValidUntilBlock
}

fn signer(seed: u8) -> Signer {
    Signer::new(
        UInt160::from_bytes(&[seed; 20]).expect("valid UInt160"),
        WitnessScope::NONE,
    )
}

#[test]
fn deserialize_unsigned_rejects_combined_fee_overflow_without_panic() {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0).unwrap(); // Version
    writer.write_u32(0x0102_0304).unwrap(); // Nonce
    writer.write_i64(i64::MAX).unwrap(); // SystemFee
    writer.write_i64(1).unwrap(); // NetworkFee

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);

    let err = Transaction::deserialize_unsigned(&mut reader)
        .expect_err("C# rejects SystemFee + NetworkFee overflow");
    assert!(
        err.to_string().contains("Invalid combined fee"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_unsigned_rejects_empty_signers_like_csharp() {
    let mut writer = BinaryWriter::new();
    write_unsigned_header(&mut writer);
    writer.write_var_int(0).unwrap();

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    let err = Transaction::deserialize_unsigned(&mut reader)
        .expect_err("C# rejects transactions without signers");

    assert!(
        err.to_string().contains("Signer count cannot be zero"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_unsigned_rejects_duplicate_signers_like_csharp() {
    let duplicate = signer(0x11);
    let mut writer = BinaryWriter::new();
    write_unsigned_header(&mut writer);
    SerializeHelper::serialize_array(&[duplicate.clone(), duplicate], &mut writer).unwrap();

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    let err = Transaction::deserialize_unsigned(&mut reader)
        .expect_err("C# rejects duplicate transaction signers");

    assert!(
        err.to_string().contains("Duplicate signer"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_unsigned_rejects_duplicate_nonrepeatable_attributes_like_csharp() {
    let mut writer = BinaryWriter::new();
    write_unsigned_header(&mut writer);
    SerializeHelper::serialize_array(&[signer(0x12)], &mut writer).unwrap();
    SerializeHelper::serialize_array(
        &[
            TransactionAttribute::HighPriority,
            TransactionAttribute::HighPriority,
        ],
        &mut writer,
    )
    .unwrap();

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    let err = Transaction::deserialize_unsigned(&mut reader)
        .expect_err("C# rejects duplicate non-repeatable attributes");

    assert!(
        err.to_string().contains("Duplicate attribute"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_unsigned_rejects_empty_script_like_csharp() {
    let mut writer = BinaryWriter::new();
    write_unsigned_header(&mut writer);
    SerializeHelper::serialize_array(&[signer(0x13)], &mut writer).unwrap();
    SerializeHelper::serialize_array::<TransactionAttribute>(&[], &mut writer).unwrap();
    writer.write_var_bytes(&[]).unwrap();

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    let err = Transaction::deserialize_unsigned(&mut reader)
        .expect_err("C# rejects empty transaction scripts");

    assert!(
        err.to_string().contains("Script length cannot be zero"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_rejects_witness_count_mismatch_like_csharp() {
    let mut writer = BinaryWriter::new();
    write_unsigned_header(&mut writer);
    SerializeHelper::serialize_array(&[signer(0x14)], &mut writer).unwrap();
    SerializeHelper::serialize_array::<TransactionAttribute>(&[], &mut writer).unwrap();
    writer.write_var_bytes(&[0x40]).unwrap();
    writer.write_var_int(0).unwrap();

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    let err = <Transaction as Serializable>::deserialize(&mut reader)
        .expect_err("C# requires witnesses == signers");

    assert!(
        err.to_string().contains("Witness count mismatch"),
        "unexpected error: {err}"
    );
}
