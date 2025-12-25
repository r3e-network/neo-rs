use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::persistence::{DataCache, StorageItem, StorageKey};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::fungible_token::PREFIX_ACCOUNT;
use neo_core::smart_contract::native::{GasToken, NativeContract, NeoToken};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{Nep6Wallet, Wallet, WalletError};
use neo_core::{UInt160, WitnessScope};
use neo_vm::{OpCode, ScriptBuilder};
use num_bigint::BigInt;
use std::sync::Arc;

fn seed_gas_balance(snapshot: &DataCache, account: &UInt160, balance: BigInt) {
    let gas = GasToken::new();
    let key = StorageKey::create_with_uint160(gas.id(), PREFIX_ACCOUNT, account);
    snapshot.add(key, StorageItem::from_bigint(balance));
}

fn build_gas_transfer_script(account: &UInt160) -> Vec<u8> {
    let gas = GasToken::new();
    let amount = BigInt::from(1);
    let mut amount_bytes = amount.to_signed_bytes_le();
    if amount_bytes.is_empty() {
        amount_bytes.push(0);
    }

    let args = vec![
        account.to_bytes(),
        account.to_bytes(),
        amount_bytes,
        Vec::new(),
    ];

    let mut builder = ScriptBuilder::new();
    for arg in args.iter().rev() {
        builder.emit_push(arg);
    }
    builder.emit_push_int(args.len() as i64);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push("transfer".as_bytes());
    builder.emit_push(&gas.hash().to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("syscall");
    builder.emit_opcode(OpCode::ASSERT);
    builder.to_array()
}

fn make_wallet_with_account() -> (Nep6Wallet, UInt160) {
    let settings = Arc::new(ProtocolSettings::default());
    let wallet = Nep6Wallet::new(Some("test".to_string()), None, Arc::clone(&settings));
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let account = rt
        .block_on(wallet.create_account(&[7u8; 32]))
        .expect("create account");
    (wallet, account.script_hash())
}

#[test]
fn test_make_transaction_custom_contracts_requires_gas_hash() {
    let (wallet, account) = make_wallet_with_account();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    seed_gas_balance(&snapshot, &account, BigInt::from(1_000_000_000_000i64));

    let mut signer = Signer::new(account, WitnessScope::CUSTOM_CONTRACTS);
    signer.allowed_contracts = vec![NeoToken::new().hash()];

    let script = build_gas_transfer_script(&account);
    let err = WalletHelper::make_transaction(
        &wallet,
        &snapshot,
        &script,
        Some(account),
        Some(&[signer]),
        None,
        &settings,
        None,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect_err("expected witness scope failure");

    assert!(matches!(err, WalletError::TransactionCreationFailed(_)));
}

#[test]
fn test_make_transaction_custom_contracts_with_gas_succeeds() {
    let (wallet, account) = make_wallet_with_account();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    seed_gas_balance(&snapshot, &account, BigInt::from(1_000_000_000_000i64));

    let mut signer = Signer::new(account, WitnessScope::CUSTOM_CONTRACTS);
    signer.allowed_contracts = vec![GasToken::new().hash()];

    let script = build_gas_transfer_script(&account);
    let tx = WalletHelper::make_transaction(
        &wallet,
        &snapshot,
        &script,
        Some(account),
        Some(&[signer]),
        None,
        &settings,
        None,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect("transaction succeeds");

    assert_eq!(tx.signers()[0].account, account);
}

#[test]
fn test_make_transaction_none_scope_fails_then_global_succeeds() {
    let (wallet, account) = make_wallet_with_account();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    seed_gas_balance(&snapshot, &account, BigInt::from(1_000_000_000_000i64));
    let script = build_gas_transfer_script(&account);

    let signer_none = Signer::new(account, WitnessScope::NONE);
    let err = WalletHelper::make_transaction(
        &wallet,
        &snapshot,
        &script,
        Some(account),
        Some(&[signer_none]),
        None,
        &settings,
        None,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect_err("expected fee-only scope failure");
    assert!(matches!(err, WalletError::TransactionCreationFailed(_)));

    let signer_global = Signer::new(account, WitnessScope::GLOBAL);
    let tx = WalletHelper::make_transaction(
        &wallet,
        &snapshot,
        &script,
        Some(account),
        Some(&[signer_global]),
        None,
        &settings,
        None,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect("global scope should succeed");
    assert_eq!(tx.signers()[0].account, account);
}

#[test]
fn test_make_transaction_missing_verification_contract_fails() {
    let settings = ProtocolSettings::default();
    let wallet = Nep6Wallet::new(Some("empty".to_string()), None, Arc::new(settings.clone()));
    let snapshot = DataCache::new(false);

    let sender = UInt160::from_bytes(&[0x01; 20]).expect("sender");
    seed_gas_balance(&snapshot, &sender, BigInt::from(1_000_000_000_000i64));

    let signer = Signer::new(sender, WitnessScope::GLOBAL);
    let script = build_gas_transfer_script(&sender);

    let err = WalletHelper::make_transaction(
        &wallet,
        &snapshot,
        &script,
        Some(sender),
        Some(&[signer]),
        None,
        &settings,
        None,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect_err("expected missing contract failure");

    assert!(matches!(err, WalletError::TransactionCreationFailed(_)));
}
