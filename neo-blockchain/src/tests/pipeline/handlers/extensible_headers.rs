use super::*;
use neo_payloads::extensible_payload::ExtensiblePayload;
use neo_payloads::header::Header;

/// C# `Blockchain.OnNewExtensiblePayload`: an extensible payload signed by
/// a whitelisted sender (here the network's validator) within its validity
/// range is accepted; a stale range or a non-whitelisted sender is rejected.
#[tokio::test]
async fn extensible_inventory_verifies_range_whitelist_and_witness() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");

    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public_key);
    let sender = neo_primitives::UInt160::from_script(&verification);

    let mut payload = ExtensiblePayload::new();
    payload.category = "dBFT".to_string();
    payload.valid_block_start = 0;
    payload.valid_block_end = 10;
    payload.sender = sender;
    payload.data = vec![0x01, 0x02, 0x03];
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&payload.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    payload.witness = neo_payloads::Witness::new_with_scripts(invocation, verification.clone());

    // Out-of-range: height 0 is not inside [5, 10) -> rejected.
    let mut stale = payload.clone();
    stale.valid_block_start = 5;
    let err = service
        .handle_extensible_inventory(stale, false)
        .await
        .expect_err("out-of-range extensible must be rejected");
    assert!(err.to_string().contains("valid range"), "{err}");

    // Non-whitelisted sender with a structurally valid witness -> rejected
    // before witness execution.
    let foreign_private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let foreign_public_key = neo_crypto::Secp256r1Crypto::derive_public_key(&foreign_private_key)
        .expect("foreign public key");
    let foreign_verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
            &foreign_public_key,
        );
    let mut foreign = ExtensiblePayload::new();
    foreign.category = payload.category.clone();
    foreign.valid_block_start = payload.valid_block_start;
    foreign.valid_block_end = payload.valid_block_end;
    foreign.sender = neo_primitives::UInt160::from_script(&foreign_verification);
    foreign.data = payload.data.clone();
    let mut foreign_sign_data = Vec::with_capacity(36);
    foreign_sign_data.extend_from_slice(&network.to_le_bytes());
    foreign_sign_data.extend_from_slice(&foreign.hash().to_bytes());
    let foreign_signature =
        neo_crypto::Secp256r1Crypto::sign(&foreign_sign_data, &foreign_private_key).expect("sign");
    let mut foreign_invocation = vec![0x0C, 64];
    foreign_invocation.extend_from_slice(&foreign_signature);
    foreign.witness =
        neo_payloads::Witness::new_with_scripts(foreign_invocation, foreign_verification);
    let err = service
        .handle_extensible_inventory(foreign, false)
        .await
        .expect_err("non-whitelisted sender must be rejected");
    assert!(err.to_string().contains("whitelist"), "{err}");

    // Valid range + whitelisted validator sender + correct signature.
    service
        .handle_extensible_inventory(payload, false)
        .await
        .expect("validly signed whitelisted extensible is accepted");
}

#[tokio::test]
async fn extensible_inventory_rejects_witness_script_hash_mismatch() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");

    let whitelisted_verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public_key);
    let sender = neo_primitives::UInt160::from_script(&whitelisted_verification);

    let foreign_private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let foreign_public_key = neo_crypto::Secp256r1Crypto::derive_public_key(&foreign_private_key)
        .expect("foreign public key");
    let foreign_verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
            &foreign_public_key,
        );

    let mut payload = ExtensiblePayload::new();
    payload.category = "dBFT".to_string();
    payload.valid_block_start = 0;
    payload.valid_block_end = 10;
    payload.sender = sender;
    payload.data = vec![0x01, 0x02, 0x03];

    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&payload.hash().to_bytes());
    let signature =
        neo_crypto::Secp256r1Crypto::sign(&sign_data, &foreign_private_key).expect("sign");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    payload.witness = neo_payloads::Witness::new_with_scripts(invocation, foreign_verification);

    let err = service
        .handle_extensible_inventory(payload, false)
        .await
        .expect_err("mismatched extensible witness must be rejected");
    assert!(
        err.to_string().contains("does not match sender"),
        "C# v3.10.1 rejects ExtensiblePayload before cache/relay when Witness.ScriptHash != Sender: {err}"
    );
}

/// C# `Blockchain.OnNewHeaders`: a header signed by the network validator
/// (over the genesis anchor's NextConsensus) is cached; a tampered witness
/// stops the batch and keeps the valid prefix (here: nothing cached).
#[tokio::test]
async fn headers_verify_against_the_anchor_next_consensus() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let invocation = |sig: &[u8]| {
        let mut script = vec![0x0C, 64];
        script.extend_from_slice(sig);
        script
    };

    // Tampered witness -> batch stops, header not cached.
    let mut tampered_sig = signature;
    tampered_sig[5] ^= 0xFF;
    let mut bad = header.clone();
    bad.witness =
        neo_payloads::Witness::new_with_scripts(invocation(&tampered_sig), verification.clone());
    service.handle_headers(vec![bad]);
    assert_eq!(
        service.header_cache.count(),
        0,
        "tampered header is not cached"
    );

    // Valid witness -> cached.
    header.witness = neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);
    service.handle_headers(vec![header]);
    assert_eq!(
        service.header_cache.count(),
        1,
        "validly signed header is cached"
    );
}

#[tokio::test]
async fn headers_in_sequence_are_accepted() {
    let (service, _handle) = fixture();
    let mut header = Header::new();
    header.set_index(1);
    service.handle_headers(vec![header]);
    assert_eq!(service.header_cache.count(), 1);
}

#[tokio::test]
async fn headers_with_gap_are_truncated() {
    let (service, _handle) = fixture();
    let mut a = Header::new();
    a.set_index(1);
    let mut b = Header::new();
    b.set_index(3); // gap on index 2
    service.handle_headers(vec![a, b]);
    assert_eq!(service.header_cache.count(), 1);
}
