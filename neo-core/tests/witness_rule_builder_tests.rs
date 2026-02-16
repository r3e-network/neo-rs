use neo_core::UInt160;
use neo_core::builders::WitnessRuleBuilder;
use neo_core::network::p2p::payloads::{WitnessCondition, WitnessRuleAction};

#[test]
fn witness_rule_builder_create() {
    let _builder = WitnessRuleBuilder::new(WitnessRuleAction::Allow);
}

#[test]
fn witness_rule_builder_condition_script_hash() {
    let rule = WitnessRuleBuilder::new(WitnessRuleAction::Allow)
        .add_condition(|wcb| {
            wcb.script_hash(UInt160::zero());
        })
        .build();

    assert_eq!(rule.action, WitnessRuleAction::Allow);
    match rule.condition {
        WitnessCondition::ScriptHash { hash } => assert_eq!(hash, UInt160::zero()),
        other => panic!("unexpected condition: {other:?}"),
    }
}

#[test]
fn witness_rule_builder_condition_and() {
    let rule = WitnessRuleBuilder::new(WitnessRuleAction::Allow)
        .add_condition(|wcb| {
            wcb.and(|and| {
                and.script_hash(UInt160::zero());
            });
        })
        .build();

    assert_eq!(rule.action, WitnessRuleAction::Allow);
    match rule.condition {
        WitnessCondition::And { conditions } => {
            assert_eq!(conditions.len(), 1);
            match &conditions[0] {
                WitnessCondition::ScriptHash { hash } => assert_eq!(*hash, UInt160::zero()),
                other => panic!("unexpected condition: {other:?}"),
            }
        }
        other => panic!("unexpected condition: {other:?}"),
    }
}
