use hex::decode as hex_decode;
use neo_core::UInt160;
use neo_core::builders::SignerBuilder;
use neo_core::cryptography::ECPoint;
use neo_core::network::p2p::payloads::{WitnessCondition, WitnessRuleAction, WitnessScope};

#[test]
fn signer_builder_new() {
    let _builder = SignerBuilder::new();
}

#[test]
fn signer_builder_sets_account() {
    let signer = SignerBuilder::new().account(UInt160::zero()).build();

    assert_eq!(signer.account, UInt160::zero());
}

#[test]
fn signer_builder_allows_contract() {
    let signer = SignerBuilder::new().allow_contract(UInt160::zero()).build();

    assert_eq!(signer.allowed_contracts, vec![UInt160::zero()]);
}

#[test]
fn signer_builder_allows_group() {
    let group = ECPoint::from_bytes(
        &hex_decode("021821807f923a3da004fb73871509d7635bcc05f41edef2a3ca5c941d8bbc1231")
            .expect("hex pubkey"),
    )
    .expect("ecpoint");

    let signer = SignerBuilder::new().allow_group(group.clone()).build();

    assert_eq!(signer.allowed_groups, vec![group]);
}

#[test]
fn signer_builder_adds_witness_scope() {
    let signer = SignerBuilder::new()
        .add_witness_scope(WitnessScope::GLOBAL)
        .build();

    assert_eq!(signer.scopes, WitnessScope::GLOBAL);
}

#[test]
fn signer_builder_adds_witness_rule() {
    let signer = SignerBuilder::new()
        .add_witness_rule(WitnessRuleAction::Allow, |cb| {
            cb.add_condition(|cond| {
                cond.script_hash(UInt160::zero());
            });
        })
        .build();

    assert_eq!(signer.rules.len(), 1);
    assert_eq!(signer.rules[0].action, WitnessRuleAction::Allow);
    match &signer.rules[0].condition {
        WitnessCondition::ScriptHash { hash } => assert_eq!(*hash, UInt160::zero()),
        other => panic!("unexpected condition: {other:?}"),
    }
}
