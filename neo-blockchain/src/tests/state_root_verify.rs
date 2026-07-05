use super::{verify_state_root, verify_state_root_with_native_provider};
use neo_config::ProtocolSettings;
use neo_payloads::Witness;
use neo_primitives::UInt256;
use neo_state_service::StateRoot;
use neo_storage::DataCache;

#[test]
fn unsigned_state_root_does_not_verify() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let sr = StateRoot::new_current(1, UInt256::from([0x11u8; 32]));
    // An unsigned root carries no witness, so there is nothing to verify.
    assert!(!verify_state_root(&sr, &settings, &snapshot));
}

#[test]
fn signed_root_without_designated_state_validators_does_not_verify() {
    let settings = ProtocolSettings::default();
    // Empty snapshot: no StateValidators are designated at any height, so
    // GetScriptHashesForVerifying yields no BFT address and verification fails —
    // the node cannot accept a "signed" root when it has no validator set to
    // check the signature against.
    let snapshot = DataCache::new(false);
    let witness = Witness::new_with_scripts(vec![0x00], vec![0x00]);
    let sr = StateRoot::new_current(1, UInt256::from([0x22u8; 32])).with_witness(witness);
    assert!(!verify_state_root(&sr, &settings, &snapshot));
}

#[test]
fn state_root_verification_exposes_explicit_native_provider_path() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let sr = StateRoot::new_current(1, UInt256::from([0x33u8; 32]));

    assert!(!verify_state_root_with_native_provider(
        &sr, &settings, &snapshot, None
    ));

    let source = include_str!("../state_root_verify.rs");
    let start = source
        .find("pub fn verify_state_root_with_native_provider")
        .expect("provider-aware state-root verifier exists");
    let verifier = &source[start..];
    assert!(
        verifier.contains("Helper::verify_witnesses_with_native_provider"),
        "state-root verification must use the explicit-provider witness helper"
    );
}
