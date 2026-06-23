use super::*;
use neo_primitives::UInt160;
use neo_serialization::json::JToken;
use neo_wallets::KeyPair;

fn assert_rule_roundtrip(rule: WitnessRule) {
    let json = rule.to_json();
    let token = JToken::parse(&json.to_string(), 128).expect("parse rule json");
    let obj = token.as_object().expect("rule object");
    let parsed = rule_from_json(obj, &ProtocolSettings::default_settings()).expect("rule parse");
    assert_eq!(parsed.to_json(), json);
}

#[test]
fn rule_from_json_roundtrip_matches_csharp_cases() {
    let action = WitnessRuleAction::Allow;

    assert_rule_roundtrip(WitnessRule::new(action, WitnessCondition::CalledByEntry));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::Or {
            conditions: vec![
                WitnessCondition::Boolean { value: true },
                WitnessCondition::Boolean { value: false },
            ],
        },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::And {
            conditions: vec![
                WitnessCondition::Boolean { value: true },
                WitnessCondition::Boolean { value: false },
            ],
        },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::Boolean { value: true },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::Not {
            condition: Box::new(WitnessCondition::Boolean { value: true }),
        },
    ));

    let keypair =
        KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p").expect("keypair");
    let group = keypair.compressed_public_key();
    let uppercase_prefixed_group = format!("0X{}", hex::encode(&group));
    assert_eq!(
        parse_group_bytes(&uppercase_prefixed_group).expect("group"),
        group
    );

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::Group {
            group: group.clone(),
        },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::CalledByContract {
            hash: UInt160::zero(),
        },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::ScriptHash {
            hash: UInt160::zero(),
        },
    ));

    assert_rule_roundtrip(WitnessRule::new(
        action,
        WitnessCondition::CalledByGroup { group },
    ));
}
