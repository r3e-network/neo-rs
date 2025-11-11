use super::super::context::WitnessConditionContext;
use super::super::dto::WitnessConditionDto;
use super::WitnessCondition;
use neo_base::hash::Hash160;
use neo_crypto::ecc256::PrivateKey;

#[test]
fn dto_roundtrip() {
    let dto = WitnessConditionDto::ScriptHash {
        hash: "0x0123456789ABCDEFFEDCBA987654321001234567".to_string(),
    };
    let condition = WitnessCondition::from_dto(dto.clone()).expect("parse");
    let back = WitnessConditionDto::from(&condition);
    assert_eq!(dto, back);
}

#[test]
fn matches_called_by_contract() {
    let hash = Hash160::from_hex_str("0x0F23456789ABCDEFFEDCBA987654321001234567").unwrap();
    let ctx = WitnessConditionContext::new(Hash160::default()).with_calling_script(hash);
    let condition = WitnessCondition::CalledByContract { hash };
    assert!(condition.matches(&ctx));
}

#[test]
fn group_condition_checks_manifest() {
    let contract_hash =
        Hash160::from_hex_str("0x0F23456789ABCDEFFEDCBA987654321001234567").unwrap();
    let private = PrivateKey::new([0x11; 32]);
    let group = private.public_key().clone();
    let groups = vec![group.clone()];
    let ctx = WitnessConditionContext::new(contract_hash).with_current_groups(&groups);
    let condition = WitnessCondition::Group { group };
    assert!(condition.matches(&ctx));
}
