use super::*;
use crate::Transaction;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256, WitnessScope};

fn sample_attributes() -> Vec<TransactionAttribute> {
    vec![
        TransactionAttribute::HighPriority,
        TransactionAttribute::OracleResponse(OracleResponse::new(
            7,
            OracleResponseCode::Success,
            vec![1, 2, 3],
        )),
        TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
        TransactionAttribute::Conflicts(Conflicts::new(UInt256::from([0xAA; 32]))),
        TransactionAttribute::NotaryAssisted(NotaryAssisted::new(2)),
    ]
}

#[test]
fn wire_mapping_preserves_attribute_type_bytes() {
    for attribute in sample_attributes() {
        let expected_type = attribute.type_id();
        let mut writer = BinaryWriter::new();

        Serializable::serialize(&attribute, &mut writer).unwrap();
        let bytes = writer.into_bytes();

        assert_eq!(bytes[0], expected_type.to_byte(), "{attribute:?}");

        let mut reader = MemoryReader::new(&bytes);
        let decoded = TransactionAttribute::deserialize_from(&mut reader).unwrap();

        assert_eq!(decoded.type_id(), expected_type);
        assert_eq!(reader.remaining(), 0);
    }
}

#[test]
fn multiplicity_matches_attribute_type_table() {
    for attribute in sample_attributes() {
        assert_eq!(
            attribute.allow_multiple(),
            attribute.type_id().allows_multiple(),
            "{attribute:?}"
        );
    }

    assert!(TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())).allow_multiple());
    assert!(!TransactionAttribute::NotaryAssisted(NotaryAssisted::new(1)).allow_multiple());
}

#[test]
fn network_fee_uses_policy_attribute_fee_like_csharp() {
    let mut tx = Transaction::new();
    tx.set_signers(vec![crate::Signer::new(
        UInt160::zero(),
        WitnessScope::NONE,
    )]);
    let attribute = TransactionAttribute::NotaryAssisted(NotaryAssisted::new(4));

    assert_eq!(
        attribute.calculate_network_fee(1_000_000, &tx),
        5_000_000,
        "C# NotaryAssisted.CalculateNetworkFee returns (NKeys + 1) * Policy.GetAttributeFeeV1"
    );
}
