use hex::decode as hex_decode;
use neo_core::builders::TransactionBuilder;
use neo_core::cryptography::ECPoint;
use neo_core::network::p2p::payloads::{
    TransactionAttribute, WitnessCondition, WitnessRuleAction, WitnessScope,
};
use neo_core::{UInt160, UInt256};
use neo_vm::op_code::OpCode;

#[test]
fn transaction_builder_create_empty() {
    let tx = TransactionBuilder::create_empty().build();
    assert_ne!(tx.hash(), UInt256::zero());
}

#[test]
fn transaction_builder_sets_version() {
    let expected = 1u8;
    let tx = TransactionBuilder::create_empty().version(expected).build();
    assert_eq!(tx.version(), expected);
}

#[test]
fn transaction_builder_sets_nonce() {
    let expected = 0x0bad_f00du32;
    let tx = TransactionBuilder::create_empty().nonce(expected).build();
    assert_eq!(tx.nonce(), expected);
}

#[test]
fn transaction_builder_sets_system_fee() {
    let expected = 42i64;
    let tx = TransactionBuilder::create_empty()
        .system_fee(expected)
        .build();
    assert_eq!(tx.system_fee(), expected);
}

#[test]
fn transaction_builder_sets_network_fee() {
    let expected = 99i64;
    let tx = TransactionBuilder::create_empty()
        .network_fee(expected)
        .build();
    assert_eq!(tx.network_fee(), expected);
}

#[test]
fn transaction_builder_sets_valid_until() {
    let expected = 123u32;
    let tx = TransactionBuilder::create_empty()
        .valid_until(expected)
        .build();
    assert_eq!(tx.valid_until_block(), expected);
}

#[test]
fn transaction_builder_attaches_script() {
    let tx = TransactionBuilder::create_empty()
        .attach_system(|sb| {
            sb.emit_opcode(OpCode::NOP);
        })
        .build();
    assert_eq!(tx.script(), &[OpCode::NOP as u8]);
}

#[test]
fn transaction_builder_adds_attributes() {
    let tx = TransactionBuilder::create_empty()
        .add_attributes(|ab| {
            ab.add_high_priority();
        })
        .build();
    assert_eq!(tx.attributes().len(), 1);
    assert!(matches!(
        tx.attributes()[0],
        TransactionAttribute::HighPriority
    ));
}

#[test]
fn transaction_builder_adds_witness() {
    let tx = TransactionBuilder::create_empty()
        .add_witness(|wb| {
            wb.add_invocation(Vec::new()).unwrap();
            wb.add_verification(Vec::new()).unwrap();
        })
        .build();
    assert_eq!(tx.witnesses().len(), 1);
    assert!(tx.witnesses()[0].invocation_script().is_empty());
    assert!(tx.witnesses()[0].verification_script().is_empty());
}

#[test]
fn transaction_builder_adds_witness_with_tx() {
    let tx = TransactionBuilder::create_empty()
        .add_witness_with_tx(|_wb, tx| {
            assert_ne!(tx.hash(), UInt256::zero());
        })
        .build();
    assert_eq!(tx.witnesses().len(), 1);
}

#[test]
fn transaction_builder_adds_signer_with_rules() {
    let expected_pubkey = ECPoint::from_bytes(
        &hex_decode("021821807f923a3da004fb73871509d7635bcc05f41edef2a3ca5c941d8bbc1231")
            .expect("hex pubkey"),
    )
    .expect("ecpoint");
    let expected_contract = UInt160::zero();

    let tx = TransactionBuilder::create_empty()
        .add_signer(|sb, _tx| {
            sb.account(expected_contract)
                .allow_contract(expected_contract)
                .allow_group(expected_pubkey.clone())
                .add_witness_scope(WitnessScope::WITNESS_RULES)
                .add_witness_rule(WitnessRuleAction::Deny, |rb| {
                    rb.add_condition(|cb| {
                        cb.script_hash(expected_contract);
                    });
                });
        })
        .build();

    assert_eq!(tx.signers().len(), 1);
    let signer = &tx.signers()[0];
    assert_eq!(signer.account, expected_contract);
    assert_eq!(signer.allowed_contracts, vec![expected_contract]);
    assert_eq!(signer.allowed_groups, vec![expected_pubkey]);
    assert_eq!(signer.scopes, WitnessScope::WITNESS_RULES);
    assert_eq!(signer.rules.len(), 1);
    assert_eq!(signer.rules[0].action, WitnessRuleAction::Deny);
    match &signer.rules[0].condition {
        WitnessCondition::ScriptHash { hash } => assert_eq!(*hash, expected_contract),
        other => panic!("unexpected condition: {other:?}"),
    }
}
