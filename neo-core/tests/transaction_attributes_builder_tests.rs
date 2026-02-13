use neo_core::builders::TransactionAttributesBuilder;
use neo_core::network::p2p::payloads::{OracleResponseCode, TransactionAttribute};
use neo_core::UInt256;

#[test]
fn transaction_attributes_builder_new() {
    let _builder = TransactionAttributesBuilder::new();
}

#[test]
fn transaction_attributes_builder_add_conflict() {
    let attrs = TransactionAttributesBuilder::new()
        .add_conflict(|cb| {
            cb.hash = UInt256::zero();
        })
        .build();

    assert_eq!(attrs.len(), 1);
    match &attrs[0] {
        TransactionAttribute::Conflicts(conflict) => {
            assert_eq!(conflict.hash, UInt256::zero());
        }
        other => panic!("unexpected attribute: {other:?}"),
    }
}

#[test]
fn transaction_attributes_builder_add_oracle_response() {
    let attrs = TransactionAttributesBuilder::new()
        .add_oracle_response(|ob| {
            ob.id = 1;
            ob.code = OracleResponseCode::Success;
            ob.result = vec![0x01, 0x02, 0x03];
        })
        .build();

    assert_eq!(attrs.len(), 1);
    match &attrs[0] {
        TransactionAttribute::OracleResponse(response) => {
            assert_eq!(response.id, 1);
            assert_eq!(response.code, OracleResponseCode::Success);
            assert_eq!(response.result, vec![0x01, 0x02, 0x03]);
        }
        other => panic!("unexpected attribute: {other:?}"),
    }
}

#[test]
fn transaction_attributes_builder_add_high_priority() {
    let attrs = TransactionAttributesBuilder::new()
        .add_high_priority()
        .build();

    assert_eq!(attrs.len(), 1);
    assert!(matches!(attrs[0], TransactionAttribute::HighPriority));
}

#[test]
fn transaction_attributes_builder_add_not_valid_before() {
    let attrs = TransactionAttributesBuilder::new()
        .add_not_valid_before(10)
        .build();

    assert_eq!(attrs.len(), 1);
    match &attrs[0] {
        TransactionAttribute::NotValidBefore(attr) => {
            assert_eq!(attr.height, 10);
        }
        other => panic!("unexpected attribute: {other:?}"),
    }
}

#[test]
#[should_panic(
    expected = "HighPriority attribute already exists. Only one allowed per transaction."
)]
fn transaction_attributes_builder_rejects_duplicate_high_priority() {
    let _attrs = TransactionAttributesBuilder::new()
        .add_high_priority()
        .add_high_priority()
        .build();
}

#[test]
#[should_panic(expected = "NotValidBefore attribute for block 10 already exists")]
fn transaction_attributes_builder_rejects_duplicate_not_valid_before() {
    let _attrs = TransactionAttributesBuilder::new()
        .add_not_valid_before(10)
        .add_not_valid_before(10)
        .build();
}
