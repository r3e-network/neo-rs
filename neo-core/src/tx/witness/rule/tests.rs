use alloc::vec::Vec;

use super::*;
use crate::tx::condition::WitnessConditionContext;
use neo_base::{encoding::SliceReader, hash::Hash160};

#[test]
fn witness_rule_json_roundtrip() {
    let hash = Hash160::from_hex_str("0x0123456789ABCDEFFEDCBA987654321001234567").unwrap();
    let rule = WitnessRule::new(Action::Allow, WitnessCondition::ScriptHash { hash });

    let json = serde_json::to_string(&rule).unwrap();
    let decoded: WitnessRule = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.action, Action::Allow);
    match decoded.condition {
        WitnessCondition::ScriptHash { hash: parsed } => assert_eq!(parsed, hash),
        _ => panic!("unexpected condition variant"),
    }
}

#[test]
fn witness_rule_binary_roundtrip() {
    let hash = Hash160::from_hex_str("0x0123456789ABCDEFFEDCBA987654321001234567").unwrap();
    let rule = WitnessRule::new(Action::Deny, WitnessCondition::ScriptHash { hash });

    let mut buf = Vec::new();
    rule.neo_encode(&mut buf);

    let mut reader = SliceReader::new(&buf);
    let decoded = WitnessRule::neo_decode(&mut reader).unwrap();
    assert_eq!(decoded.action, rule.action);
    assert_eq!(decoded.condition, rule.condition);
}

#[test]
fn witness_rule_matches_context() {
    let hash = Hash160::from_hex_str("0x0123456789ABCDEFFEDCBA987654321001234567").unwrap();
    let ctx = WitnessConditionContext::new(hash);
    let allow = WitnessRule::new(Action::Allow, WitnessCondition::ScriptHash { hash });
    assert_eq!(allow.evaluate(&ctx), Some(true));

    let deny = WitnessRule::new(Action::Deny, WitnessCondition::ScriptHash { hash });
    assert_eq!(deny.evaluate(&ctx), Some(false));
}
