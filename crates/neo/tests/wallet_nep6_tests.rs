use std::fs;
use std::sync::Arc;

use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::wallets::wallet::WalletError;
use neo_core::wallets::wallet::WalletResult;
use neo_core::wallets::{KeyPair, Nep6Wallet, Wallet};
use neo_core::WitnessScope;
use neo_vm::op_code::OpCode;
use rand::RngCore;
use tokio::runtime::Runtime;

fn runtime() -> Runtime {
    Runtime::new().expect("tokio runtime")
}

fn temp_wallet_path() -> String {
    let mut rng = rand::thread_rng();
    let mut suffix = [0u8; 8];
    rng.fill_bytes(&mut suffix);
    let filename = format!("nep6_test_{}.json", hex::encode(suffix));
    std::env::temp_dir()
        .join(filename)
        .to_string_lossy()
        .to_string()
}

#[test]
fn nep6_wallet_imports_and_signs() -> WalletResult<()> {
    let settings = Arc::new(ProtocolSettings::default());
    let wallet_path = temp_wallet_path();
    let mut wallet = Nep6Wallet::new(
        Some("test".to_string()),
        Some(wallet_path.clone()),
        settings,
    );

    let key_pair = KeyPair::generate().expect("key generation failed");
    let wif = key_pair.to_wif();
    let script_hash = key_pair.get_script_hash();

    let rt = runtime();
    let _account = rt.block_on(wallet.import_wif(&wif)).expect("import WIF");

    // Sign raw data
    let payload = b"neo-rs-wallet";
    let signature = rt
        .block_on(wallet.sign(payload, &script_hash))
        .expect("sign data");
    let original_key = KeyPair::from_wif(&wif).expect("wif decode");
    assert!(original_key
        .verify(payload, &signature)
        .expect("verify signature"));

    // Sign transaction and ensure witness is produced
    let mut transaction = Transaction::new();
    transaction.set_script(vec![OpCode::PUSH1 as u8]);
    transaction.set_valid_until_block(1);
    transaction.add_signer(Signer::new(script_hash, WitnessScope::CALLED_BY_ENTRY));
    rt.block_on(wallet.sign_transaction(&mut transaction))
        .expect("sign transaction");

    assert_eq!(transaction.witnesses().len(), transaction.signers().len());
    let witness = &transaction.witnesses()[0];
    let verification_script =
        ContractHelper::signature_redeem_script(&original_key.compressed_public_key());
    assert_eq!(witness.verification_script, verification_script);

    fs::remove_file(wallet_path).ok();
    Ok(())
}

#[test]
fn nep6_wallet_changes_password_and_unlocks() -> WalletResult<()> {
    let settings = Arc::new(ProtocolSettings::default());
    let wallet_path = temp_wallet_path();
    let mut wallet = Nep6Wallet::new(
        Some("test".to_string()),
        Some(wallet_path.clone()),
        settings,
    );

    let key_pair = KeyPair::generate().expect("key generation failed");
    let old_password = "old-secret";
    let new_password = "new-secret";
    let nep2 = key_pair.to_nep2(old_password).expect("nep2 export");
    let script_hash = key_pair.get_script_hash();

    let rt = runtime();
    rt.block_on(wallet.import_nep2(&nep2, old_password))
        .expect("import nep2");

    // Change password
    rt.block_on(wallet.change_password(old_password, new_password))
        .expect("change password");

    // Unlock using new password succeeds
    assert!(rt.block_on(wallet.unlock(new_password))?);

    // Signing succeeds after unlock
    let payload = b"neo-change-password";
    let signature = rt.block_on(wallet.sign(payload, &script_hash))?;
    assert!(key_pair.verify(payload, &signature).expect("verify"));

    // Unlocking with old password should now fail
    assert!(matches!(
        rt.block_on(wallet.unlock(old_password)),
        Err(WalletError::InvalidPassword)
    ));

    // verify_password with new password returns true, old returns false
    assert!(rt.block_on(wallet.verify_password(new_password))?);
    assert!(!rt.block_on(wallet.verify_password(old_password))?);

    fs::remove_file(wallet_path).ok();
    Ok(())
}
