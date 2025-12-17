use neo_core::neo_io::{MemoryReader, Serializable, SerializableExt};
use neo_core::{UInt160, WitnessCondition, WitnessRule, WitnessRuleAction};
use serde_json::Value;
use std::str::FromStr;

fn group_bytes() -> Vec<u8> {
    // Same secp256r1 point used across the C# unit tests.
    let encoded = hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
        .expect("hex");
    let point =
        neo_core::ECPoint::decode(&encoded, neo_core::ECCurve::secp256r1()).expect("valid ECPoint");
    point.encode_point(true).expect("compress")
}

#[test]
fn csharp_ut_witness_condition_from_json_1() {
    let hash = UInt160::zero();
    let point = group_bytes();
    let condition = WitnessCondition::Or {
        conditions: vec![
            WitnessCondition::CalledByContract { hash },
            WitnessCondition::CalledByGroup {
                group: point.clone(),
            },
        ],
    };

    let json = condition.to_json();
    let parsed = WitnessCondition::from_json_with_depth(&json, 2).expect("parse");

    let WitnessCondition::Or { conditions } = parsed else {
        panic!("expected OrCondition");
    };
    assert_eq!(conditions.len(), 2);
    assert!(matches!(
        conditions[0],
        WitnessCondition::CalledByContract { .. }
    ));
    assert!(matches!(
        conditions[1],
        WitnessCondition::CalledByGroup { .. }
    ));

    let WitnessCondition::CalledByContract { hash: parsed_hash } = &conditions[0] else {
        unreachable!();
    };
    let WitnessCondition::CalledByGroup {
        group: parsed_group,
    } = &conditions[1]
    else {
        unreachable!();
    };
    assert_eq!(*parsed_hash, hash);
    assert_eq!(*parsed_group, point);
}

#[test]
fn csharp_ut_witness_condition_from_json_2() {
    let point = group_bytes();
    let hash1 = UInt160::zero();
    let hash2 = UInt160::from_str("0xd2a4cff31913016155e38e474a2c06d08be276cf").expect("hash2");

    let jstr = "{\"type\":\"Or\",\"expressions\":[{\"type\":\"And\",\"expressions\":[{\"type\":\"CalledByContract\",\"hash\":\"0x0000000000000000000000000000000000000000\"},{\"type\":\"ScriptHash\",\"hash\":\"0xd2a4cff31913016155e38e474a2c06d08be276cf\"}]},{\"type\":\"Or\",\"expressions\":[{\"type\":\"CalledByGroup\",\"group\":\"03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c\"},{\"type\":\"Boolean\",\"expression\":true}]}]}";
    let json: Value = serde_json::from_str(jstr).expect("json");

    let parsed = WitnessCondition::from_json_with_depth(&json, WitnessCondition::MAX_NESTING_DEPTH)
        .expect("parse");

    let WitnessCondition::Or { conditions } = parsed else {
        panic!("expected OrCondition");
    };
    assert_eq!(conditions.len(), 2);

    let WitnessCondition::And { conditions: ands } = &conditions[0] else {
        panic!("expected AndCondition");
    };
    let WitnessCondition::Or { conditions: ors } = &conditions[1] else {
        panic!("expected OrCondition");
    };

    assert_eq!(ands.len(), 2);
    assert_eq!(ors.len(), 2);

    let WitnessCondition::CalledByContract { hash: cbcc_hash } = &ands[0] else {
        unreachable!();
    };
    assert_eq!(*cbcc_hash, hash1);
    let WitnessCondition::ScriptHash { hash: sh_hash } = &ands[1] else {
        panic!("expected ScriptHash");
    };
    assert_eq!(*sh_hash, hash2);

    let WitnessCondition::CalledByGroup { group } = &ors[0] else {
        panic!("expected CalledByGroup");
    };
    let WitnessCondition::Boolean { value } = &ors[1] else {
        panic!("expected Boolean");
    };
    assert_eq!(*group, point);
    assert!(*value);
}

#[test]
fn csharp_ut_witness_condition_nesting_binary_roundtrip_and_overflow() {
    let nested_or = WitnessCondition::Or {
        conditions: vec![WitnessCondition::Or {
            conditions: vec![WitnessCondition::Boolean { value: true }],
        }],
    };
    let bytes = nested_or.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        WitnessCondition::deserialize_with_depth(&mut reader, WitnessCondition::MAX_NESTING_DEPTH)
            .expect("deserialize");
    assert_eq!(nested_or, decoded);

    let nested_and = WitnessCondition::And {
        conditions: vec![WitnessCondition::And {
            conditions: vec![WitnessCondition::Boolean { value: true }],
        }],
    };
    let bytes = nested_and.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        WitnessCondition::deserialize_with_depth(&mut reader, WitnessCondition::MAX_NESTING_DEPTH)
            .expect("deserialize");
    assert_eq!(nested_and, decoded);

    let nested_not = WitnessCondition::Not {
        condition: Box::new(WitnessCondition::Not {
            condition: Box::new(WitnessCondition::Boolean { value: true }),
        }),
    };
    let bytes = nested_not.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        WitnessCondition::deserialize_with_depth(&mut reader, WitnessCondition::MAX_NESTING_DEPTH)
            .expect("deserialize");
    assert_eq!(nested_not, decoded);

    let overflow_or = WitnessCondition::Or {
        conditions: vec![WitnessCondition::Or {
            conditions: vec![WitnessCondition::Or {
                conditions: vec![WitnessCondition::Boolean { value: true }],
            }],
        }],
    };
    let bytes = overflow_or.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(WitnessCondition::deserialize_with_depth(
        &mut reader,
        WitnessCondition::MAX_NESTING_DEPTH
    )
    .is_err());

    let overflow_and = WitnessCondition::And {
        conditions: vec![WitnessCondition::And {
            conditions: vec![WitnessCondition::And {
                conditions: vec![WitnessCondition::Boolean { value: true }],
            }],
        }],
    };
    let bytes = overflow_and.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(WitnessCondition::deserialize_with_depth(
        &mut reader,
        WitnessCondition::MAX_NESTING_DEPTH
    )
    .is_err());

    let overflow_not = WitnessCondition::Not {
        condition: Box::new(WitnessCondition::Not {
            condition: Box::new(WitnessCondition::Not {
                condition: Box::new(WitnessCondition::Boolean { value: true }),
            }),
        }),
    };
    let bytes = overflow_not.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(WitnessCondition::deserialize_with_depth(
        &mut reader,
        WitnessCondition::MAX_NESTING_DEPTH
    )
    .is_err());
}

#[test]
fn witness_group_condition_wire_format_matches_csharp() {
    let group = group_bytes();
    let condition = WitnessCondition::Group {
        group: group.clone(),
    };

    let bytes = condition.to_array().expect("serialize");
    assert_eq!(bytes.len(), 1 + 33);
    assert_eq!(bytes[0], 0x19); // WitnessConditionType::Group
    assert_eq!(&bytes[1..], group.as_slice());

    let mut reader = MemoryReader::new(&bytes);
    let decoded = <WitnessCondition as Serializable>::deserialize(&mut reader).expect("decode");
    assert_eq!(decoded, condition);
}

#[test]
fn csharp_ut_witness_rule_equatable() {
    let expected = WitnessRule::new(
        WitnessRuleAction::Allow,
        WitnessCondition::Boolean { value: true },
    );
    let actual = WitnessRule::new(
        WitnessRuleAction::Allow,
        WitnessCondition::Boolean { value: true },
    );
    let not_equal = WitnessRule::new(
        WitnessRuleAction::Deny,
        WitnessCondition::Boolean { value: false },
    );

    assert_eq!(expected, actual);
    assert_ne!(expected, not_equal);
}
