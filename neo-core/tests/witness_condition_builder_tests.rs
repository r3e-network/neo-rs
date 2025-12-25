use hex::decode as hex_decode;
use neo_core::builders::{WitnessConditionBuilder};
use neo_core::cryptography::ECPoint;
use neo_core::network::p2p::payloads::WitnessCondition;
use neo_core::UInt160;

fn sample_pubkey() -> ECPoint {
    ECPoint::from_bytes(
        &hex_decode("021821807f923a3da004fb73871509d7635bcc05f41edef2a3ca5c941d8bbc1231")
            .expect("hex pubkey"),
    )
    .expect("ecpoint")
}

#[test]
fn witness_condition_builder_and_condition() {
    let expected_pubkey = sample_pubkey();
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .and(|and| {
            and.called_by_contract(expected_contract);
            and.called_by_group(expected_pubkey.clone());
        })
        .build();

    match condition {
        WitnessCondition::And { conditions } => {
            assert_eq!(conditions.len(), 2);
            match &conditions[0] {
                WitnessCondition::CalledByContract { hash } => assert_eq!(*hash, expected_contract),
                other => panic!("unexpected condition: {other:?}"),
            }
            match &conditions[1] {
                WitnessCondition::CalledByGroup { group } => {
                    assert_eq!(group, expected_pubkey.as_bytes());
                }
                other => panic!("unexpected condition: {other:?}"),
            }
        }
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_or_condition() {
    let expected_pubkey = sample_pubkey();
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .or(|or| {
            or.called_by_contract(expected_contract);
            or.called_by_group(expected_pubkey.clone());
        })
        .build();

    match condition {
        WitnessCondition::Or { conditions } => {
            assert_eq!(conditions.len(), 2);
            match &conditions[0] {
                WitnessCondition::CalledByContract { hash } => assert_eq!(*hash, expected_contract),
                other => panic!("unexpected condition: {other:?}"),
            }
            match &conditions[1] {
                WitnessCondition::CalledByGroup { group } => {
                    assert_eq!(group, expected_pubkey.as_bytes());
                }
                other => panic!("unexpected condition: {other:?}"),
            }
        }
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_boolean() {
    let condition = WitnessConditionBuilder::create().boolean(true).build();
    match condition {
        WitnessCondition::Boolean { value } => assert!(value),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_called_by_contract() {
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .called_by_contract(expected_contract)
        .build();

    match condition {
        WitnessCondition::CalledByContract { hash } => assert_eq!(hash, expected_contract),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_called_by_entry() {
    let condition = WitnessConditionBuilder::create().called_by_entry().build();
    match condition {
        WitnessCondition::CalledByEntry => {}
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_called_by_group() {
    let expected_pubkey = sample_pubkey();
    let condition = WitnessConditionBuilder::create()
        .called_by_group(expected_pubkey.clone())
        .build();

    match condition {
        WitnessCondition::CalledByGroup { group } => {
            assert_eq!(group, expected_pubkey.as_bytes());
        }
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_group() {
    let expected_pubkey = sample_pubkey();
    let condition = WitnessConditionBuilder::create()
        .group(expected_pubkey.clone())
        .build();

    match condition {
        WitnessCondition::Group { group } => assert_eq!(group, expected_pubkey.as_bytes()),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_script_hash() {
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .script_hash(expected_contract)
        .build();

    match condition {
        WitnessCondition::ScriptHash { hash } => assert_eq!(hash, expected_contract),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_defaults_to_true() {
    let condition = WitnessConditionBuilder::create().build();
    match condition {
        WitnessCondition::Boolean { value } => assert!(value),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_not_with_and() {
    let expected_pubkey = sample_pubkey();
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .not(|not| {
            not.and(|and| {
                and.called_by_contract(expected_contract);
                and.called_by_group(expected_pubkey.clone());
            });
        })
        .build();

    match condition {
        WitnessCondition::Not { condition } => match *condition {
            WitnessCondition::And { conditions } => {
                assert_eq!(conditions.len(), 2);
                match &conditions[0] {
                    WitnessCondition::CalledByContract { hash } => {
                        assert_eq!(*hash, expected_contract)
                    }
                    other => panic!("unexpected condition: {other:?}"),
                }
                match &conditions[1] {
                    WitnessCondition::CalledByGroup { group } => {
                        assert_eq!(group, expected_pubkey.as_bytes())
                    }
                    other => panic!("unexpected condition: {other:?}"),
                }
            }
            other => panic!("unexpected condition: {other:?}"),
        },
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_condition_builder_not_with_or() {
    let expected_pubkey = sample_pubkey();
    let expected_contract = UInt160::zero();
    let condition = WitnessConditionBuilder::create()
        .not(|not| {
            not.or(|or| {
                or.called_by_contract(expected_contract);
                or.called_by_group(expected_pubkey.clone());
            });
        })
        .build();

    match condition {
        WitnessCondition::Not { condition } => match *condition {
            WitnessCondition::Or { conditions } => {
                assert_eq!(conditions.len(), 2);
                match &conditions[0] {
                    WitnessCondition::CalledByContract { hash } => {
                        assert_eq!(*hash, expected_contract)
                    }
                    other => panic!("unexpected condition: {other:?}"),
                }
                match &conditions[1] {
                    WitnessCondition::CalledByGroup { group } => {
                        assert_eq!(group, expected_pubkey.as_bytes())
                    }
                    other => panic!("unexpected condition: {other:?}"),
                }
            }
            other => panic!("unexpected condition: {other:?}"),
        },
        other => panic!("unexpected condition: {other:?}"),
    }
}
