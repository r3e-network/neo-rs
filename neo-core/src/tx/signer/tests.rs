use super::Signer;
use crate::tx::{
    condition::WitnessConditionContext, Action, WitnessCondition, WitnessRule, WitnessScope,
    WitnessScopes,
};
use neo_base::hash::Hash160;

#[test]
fn global_signer_allows_anything() {
    let signer = Signer {
        account: H160::default(),
        scopes: WitnessScopes::from_bits(WitnessScope::Global as u8),
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };
    let ctx = WitnessConditionContext::new(Hash160::default());
    assert!(signer.allows(&ctx));
}

#[test]
fn custom_contract_rule_matches_context() {
    let hash = Hash160::from_hex_str("0x0123456789ABCDEFFEDCBA987654321001234567").expect("hash");
    let signer = Signer {
        account: hash,
        scopes: {
            let mut scopes = WitnessScopes::new();
            scopes.add_scope(WitnessScope::CustomContracts);
            scopes
        },
        allowed_contract: vec![hash],
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };
    let ctx = WitnessConditionContext::new(hash);
    assert!(signer.allows(&ctx));
}

#[test]
fn deny_rule_short_circuits() {
    let hash = Hash160::from_hex_str("0x0F23456789ABCDEFFEDCBA987654321001234567").expect("hash");
    let rule = WitnessRule::new(Action::Deny, WitnessCondition::ScriptHash { hash });
    let signer = Signer {
        account: hash,
        scopes: {
            let mut scopes = WitnessScopes::new();
            scopes.add_scope(WitnessScope::WitnessRules);
            scopes
        },
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: vec![rule],
    };
    let ctx = WitnessConditionContext::new(hash);
    assert!(!signer.allows(&ctx));
}
