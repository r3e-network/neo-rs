use super::controller::WalletConsensusSigner;
use super::*;
use neo_consensus::{ConsensusMessageType, ConsensusSigner};
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_protocol::ProtocolSettings;
use neo_wallets::{KeyPair, Nep6Wallet};

#[tokio::test(flavor = "multi_thread")]
async fn consensus_signs_with_the_exact_wallet_validator_key() {
    let settings = Arc::new(ProtocolSettings::default());
    let wallet = Arc::new(Nep6Wallet::create(
        Some("consensus-wallet-signing".to_string()),
        None,
        settings,
    ));
    let key_pair = KeyPair::from_private_key(&[0x31; 32]).expect("consensus key");
    let account = key_pair.get_script_hash();
    wallet.create_account(&[0x31; 32]).expect("wallet account");
    let wallet_trait: Arc<dyn Wallet> = wallet;
    let signer = WalletConsensusSigner::new(wallet_trait);
    let context = neo_consensus::ConsensusSignContext {
        block_index: 42,
        view_number: 3,
        message_type: ConsensusMessageType::Commit,
        proposal_digest: Some(UInt256::from([0x42; 32])),
        commit_block_hash: true,
    };
    let data = [1, 2, 3, 4];

    assert!(signer.can_sign(&account));
    let signature = signer
        .sign(&data, &account, &context)
        .expect("consensus signature");
    let signature: [u8; 64] = signature.try_into().expect("64-byte P-256 signature");
    assert!(
        Secp256r1Crypto::verify_prehash(
            &Crypto::sha256(&data),
            &signature,
            &key_pair.compressed_public_key(),
        )
        .expect("signature verification")
    );
}

#[test]
fn consensus_rejects_an_account_missing_from_the_active_wallet() {
    let settings = Arc::new(ProtocolSettings::default());
    let wallet = Arc::new(Nep6Wallet::create(
        Some("consensus-missing-account".to_string()),
        None,
        settings,
    ));
    let wallet_trait: Arc<dyn Wallet> = wallet;
    let signer = WalletConsensusSigner::new(wallet_trait);
    let missing = KeyPair::from_private_key(&[0x32; 32])
        .expect("missing validator key")
        .get_script_hash();
    let context = neo_consensus::ConsensusSignContext {
        block_index: 43,
        view_number: 1,
        message_type: ConsensusMessageType::Commit,
        proposal_digest: Some(UInt256::from([0x43; 32])),
        commit_block_hash: true,
    };

    assert!(!signer.can_sign(&missing));
    signer
        .sign(&[4, 3, 2, 1], &missing, &context)
        .expect_err("a missing wallet account must fail closed");
}

#[cfg(all(feature = "tee", not(feature = "tee-sgx")))]
#[tokio::test(flavor = "multi_thread")]
async fn tee_ordered_candidates_fail_closed_against_local_view() {
    // Both the privacy-mode path and the diagnostics TEE path produce
    // candidates from the TEE ordered hash table strictly intersected
    // with the bounded local-only isolation view. A hash the TEE ordered
    // but the local pool never verified must never become a candidate.
    let system =
        neo_system::NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let pool = system.mempool();

    let temp = tempfile::tempdir().expect("temp dir");
    let runtime = crate::tee_integration::TeeRuntime::with_privacy(
        temp.path().to_path_buf(),
        "fcfs",
        1000,
        crate::tee_integration::PrivacyEncryption::TeeSealedKey,
    )
    .expect("privacy runtime");

    let tx_hash = [9u8; 32];
    runtime
        .mempool
        .add_transaction(
            tx_hash,
            b"consensus-privacy-candidates".to_vec(),
            1_000,
            0,
            [7u8; 20],
        )
        .expect("add encrypted entry");
    let summary = runtime.mempool.resolve_pending();
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.rejected, 0);
    // The TEE now has the hash ordered...
    assert_eq!(runtime.mempool.get_ordered_hashes(10), vec![tx_hash]);

    // ...but the local isolation view never verified it, so the
    // intersection is empty and no candidate is produced.
    let candidates = MempoolCandidateSource::tee_ordered_candidates(&runtime, &pool, 10);
    assert!(
        candidates.is_empty(),
        "TEE-ordered hashes must fail closed against the local isolation view"
    );

    runtime.shutdown().expect("runtime shutdown");
    system.shutdown().await.expect("system shutdown");
}
