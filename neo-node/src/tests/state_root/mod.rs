//! Tests for the node-level StateService driver codec and sender rotation.

use super::*;
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_io::{MemoryReader, Serializable, SerializableExtensions};
use neo_payloads::Witness;
use neo_primitives::{UInt160, UInt256};
use neo_state_service::{MessageType, STATE_SERVICE_CATEGORY, StateRoot, Vote};
use neo_vm::script_builder::RedeemScript;

const NETWORK: u32 = 0x4E45_4F4E;

fn test_keypair() -> ([u8; 32], ECPoint) {
    let mut sk = [0u8; 32];
    sk[31] = 7;
    let pk = Secp256r1Crypto::derive_public_key(&sk).expect("derive pubkey");
    (sk, ECPoint::from_bytes(&pk).expect("ecpoint"))
}

#[test]
fn vote_extensible_round_trips_through_the_codec() {
    let (sk, pk) = test_keypair();
    let vote = Vote {
        validator_index: 2,
        root_index: 42,
        signature: vec![0x11; 64],
    };
    let bytes = vote.to_array().expect("vote bytes");
    let ext = build_extensible(
        MessageType::Vote,
        &bytes,
        vote.root_index,
        VOTE_VALID_BLOCK_END_THRESHOLD,
        &sk,
        &pk,
        NETWORK,
    )
    .expect("build vote extensible");

    assert_eq!(ext.category, STATE_SERVICE_CATEGORY);
    assert_eq!(ext.valid_block_start, vote.root_index);
    assert_eq!(
        ext.valid_block_end,
        vote.root_index + VOTE_VALID_BLOCK_END_THRESHOLD
    );
    // Sender is the signer's signature-redeem-script hash, matching the witness.
    let redeem = RedeemScript::signature_redeem_script(pk.as_bytes());
    assert_eq!(ext.sender, UInt160::from_script(&redeem));
    assert_eq!(ext.witness.verification_script, redeem);

    let (message_type, body) = decode_message(&ext).expect("decode");
    assert_eq!(message_type, MessageType::Vote);
    let decoded = Vote::deserialize(&mut MemoryReader::new(body)).expect("decode vote");
    assert_eq!(decoded.validator_index, vote.validator_index);
    assert_eq!(decoded.root_index, vote.root_index);
    assert_eq!(decoded.signature, vote.signature);
}

#[test]
fn state_root_extensible_round_trips_through_the_codec() {
    let (sk, pk) = test_keypair();
    let root = StateRoot::new_current(9, UInt256::from([0xCDu8; 32]))
        .with_witness(Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]));
    let ext = build_extensible(
        MessageType::StateRoot,
        &root.to_array(),
        root.index(),
        STATE_ROOT_VALID_BLOCK_END_THRESHOLD,
        &sk,
        &pk,
        NETWORK,
    )
    .expect("build state root extensible");

    let (message_type, body) = decode_message(&ext).expect("decode");
    assert_eq!(message_type, MessageType::StateRoot);
    let decoded = StateRoot::deserialize(&mut MemoryReader::new(body)).expect("decode root");
    assert_eq!(decoded.index(), root.index());
    assert_eq!(decoded.root_hash(), root.root_hash());
    let witness = decoded.witness().expect("witness survives round trip");
    assert_eq!(witness.invocation_script, vec![1, 2, 3]);
    assert_eq!(witness.verification_script, vec![4, 5, 6]);
}

#[test]
fn a_non_state_service_extensible_is_not_decoded() {
    let (sk, pk) = test_keypair();
    let mut ext = build_extensible(
        MessageType::Vote,
        &Vote {
            validator_index: 0,
            root_index: 1,
            signature: vec![0u8; 64],
        }
        .to_array()
        .unwrap(),
        1,
        VOTE_VALID_BLOCK_END_THRESHOLD,
        &sk,
        &pk,
        NETWORK,
    )
    .unwrap();
    ext.category = "dBFT".to_string();
    assert!(decode_message(&ext).is_none());
}

#[test]
fn sender_rotates_backward_with_each_retry() {
    // N = 7, root_index = 10: sender = (10 - retries) mod 7.
    assert_eq!(StateRootDriver::sender_index(10, 0, 7), 3);
    assert_eq!(StateRootDriver::sender_index(10, 1, 7), 2);
    assert_eq!(StateRootDriver::sender_index(10, 4, 7), 6); // (10-4)=6
    // Wraps past zero without panicking.
    assert_eq!(StateRootDriver::sender_index(2, 5, 7), 4); // (2-5)=-3 -> 4
}

#[test]
fn driver_verifies_signed_roots_with_explicit_native_provider() {
    let source = include_str!("../../state_root/driver.rs");
    assert!(
        source.contains("native_contract_provider: Arc<dyn NativeContractProvider>"),
        "StateRootDriver must own the native provider captured at node startup"
    );
    assert!(
        source.contains("verify_state_root_with_native_provider"),
        "inbound signed StateRoot verification must use the explicit-provider verifier"
    );
    assert!(
        source.contains("state_root_verifiers_with_native_provider"),
        "StateRootDriver must read StateValidator designations through the shared explicit-provider helper"
    );
    assert!(
        source.contains("Arc::clone(&self.native_contract_provider)"),
        "StateRootDriver must pass its captured provider into state-root verification"
    );
    assert!(
        !source.contains("Some(Arc::clone(&self.native_contract_provider))"),
        "StateRootDriver must pass the captured provider directly, not through an optional ambient-provider path"
    );
    assert!(
        !source.contains("NativeStateRootProviderFactory"),
        "StateRootDriver must not depend on a private state-root native provider factory"
    );
    assert!(
        !source.contains("RoleManagement::new()"),
        "StateRootDriver must not construct RoleManagement directly"
    );
    let verifier = include_str!("../../../../neo-blockchain/src/state_root_verify.rs");
    assert!(verifier.contains("trait StateRootNativeProvider"));
    assert!(
        verifier.contains("StateRootNativeProviderAdapter"),
        "state-root verification should adapt the node-composed native provider"
    );
    assert!(
        verifier.contains("get_native_contract_by_name(\"RoleManagement\")"),
        "state-root verification should resolve RoleManagement through the explicit native provider"
    );
    assert!(!verifier.contains("trait StateRootNativeProviderFactory"));
    assert!(!verifier.contains("struct NativeStateRootProviderFactory"));
    assert!(!verifier.contains("RoleManagement::new()"));
}
