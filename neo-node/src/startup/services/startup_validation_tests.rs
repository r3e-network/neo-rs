use super::*;
use crate::config::DbftSettings;
use crate::consensus::ensure_wallet_can_validate;
use crate::wallet_provider::NodeWalletProvider;
use neo_protocol::ProtocolSettings;
use neo_wallets::Wallet as CoreWallet;
use neo_wallets::{KeyPair, Nep6Wallet};

fn dbft_settings(network: u32) -> Option<DbftSettings> {
    Some(DbftSettings {
        recovery_logs: "ConsensusState".to_string(),
        ignore_recovery_logs: false,
        auto_start: false,
        network,
        max_block_size: 65_536,
        max_block_system_fee: 150_000_000_000,
        exception_policy: neo_primitives::UnhandledExceptionPolicy::StopNode,
    })
}

fn validator_settings(private_key: &[u8; 32]) -> ProtocolSettings {
    let validator = KeyPair::from_private_key(private_key)
        .expect("validator key")
        .get_public_key_point()
        .expect("validator public key");
    let mut settings = ProtocolSettings::default();
    settings.network = 42;
    settings.validators_count = 1;
    settings.standby_committee = vec![validator];
    settings
}

fn wallet(settings: &ProtocolSettings, private_key: &[u8; 32]) -> Arc<Nep6Wallet> {
    let wallet = Arc::new(Nep6Wallet::create(
        Some("dbft-startup-test".to_string()),
        None,
        Arc::new(settings.clone()),
    ));
    wallet.create_account(private_key).expect("wallet account");
    wallet
}

#[tokio::test(flavor = "multi_thread")]
async fn dbft_startup_rejects_missing_provider_wallet_and_network_mismatch() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
    let configured = dbft_settings(settings.network);

    let error = maybe_enable_dbft_consensus(&configured, &settings, &system, None)
        .expect_err("missing provider must fail");
    assert!(error.to_string().contains("requires a wallet provider"));

    let provider = Arc::new(NodeWalletProvider::new());
    let error = maybe_enable_dbft_consensus(&configured, &settings, &system, Some(&provider))
        .expect_err("missing wallet must fail");
    assert!(error.to_string().contains("requires a loaded wallet"));

    let mismatched = dbft_settings(settings.network.wrapping_add(1));
    let error = maybe_enable_dbft_consensus(&mismatched, &settings, &system, None)
        .expect_err("network mismatch must fail before provider setup");
    assert!(error.to_string().contains("network mismatch"));

    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn dbft_startup_rejects_a_wallet_without_the_next_validator_key() {
    let settings = validator_settings(&[0x41; 32]);
    let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
    let provider = Arc::new(NodeWalletProvider::new());
    provider.set_wallet(Some(wallet(&settings, &[0x42; 32])));

    let error = maybe_enable_dbft_consensus(
        &dbft_settings(settings.network),
        &settings,
        &system,
        Some(&provider),
    )
    .expect_err("a non-validator key must not start dBFT");
    assert!(
        error
            .to_string()
            .contains("requires an unlocked validator key")
    );

    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn dbft_startup_accepts_the_next_validator_key_even_without_auto_start() {
    let settings = validator_settings(&[0x43; 32]);
    let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
    let wallet: Arc<dyn CoreWallet> = wallet(&settings, &[0x43; 32]);
    ensure_wallet_can_validate(&system, &wallet).expect("shared validator preflight");
    let provider = Arc::new(NodeWalletProvider::new());
    provider.set_wallet(Some(wallet));

    maybe_enable_dbft_consensus(
        &dbft_settings(settings.network),
        &settings,
        &system,
        Some(&provider),
    )
    .expect("manual-start validator mode still requires and accepts signing authority");

    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn contract_management_integrity_validates_the_native_snapshot() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    validate_contract_management_integrity(&system).expect("native snapshot integrity");
    system.shutdown().await.expect("shutdown");
}
