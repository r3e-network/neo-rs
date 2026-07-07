use super::support::signature_contract_pubkey;
use super::*;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server_settings::RpcServerConfig;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_config::ProtocolSettings;
use neo_crypto::Secp256r1Crypto;
use neo_execution::helper::Helper as ContractHelper;
use neo_native_contracts::{GasToken, LedgerContract, NeoToken};
use neo_payloads::VerifyResult;
use neo_payloads::Witness;
use neo_payloads::conflicts::Conflicts;
use neo_payloads::get_sign_data_vec;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_primitives::{UInt256, WitnessScope};
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::{OpCode, VmState as VMState};
use neo_wallets::wallet_helper::WalletAddress as wallet_helper;
use neo_wallets::{KeyPair, Nep6Wallet, WalletError};
use num_bigint::BigInt;
use serde_json::{Value, json};
use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::{Handle, Runtime};

fn temp_wallet_path() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("timestamp")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("rpc_wallet_{nanos}.json"))
        .to_string_lossy()
        .to_string()
}

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

async fn create_wallet_file(password: &str) -> (String, KeyPair, String) {
    let settings = Arc::new(ProtocolSettings::default());
    let path = temp_wallet_path();
    let wallet = Nep6Wallet::new(
        Some("rpc-wallet".to_string()),
        Some(path.clone()),
        settings.clone(),
    );
    let keypair = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair");
    let nep2 = keypair
        .to_nep2(password, settings.address_version)
        .expect("nep2");
    wallet
        .import_nep2(&nep2, password)
        .await
        .expect("import nep2");
    wallet.persist().expect("persist wallet");
    let address = wallet_helper::to_address(&keypair.script_hash(), settings.address_version);
    (path, keypair, address)
}

fn make_authenticated_server() -> RpcServer {
    make_authenticated_server_with_max_fee(RpcServerConfig::default().max_fee)
}

fn authenticated_config() -> RpcServerConfig {
    RpcServerConfig {
        rpc_user: "user".to_string(),
        rpc_pass: "pass".to_string(),
        ..Default::default()
    }
}

fn authenticated_config_with_max_fee(max_fee: i64) -> RpcServerConfig {
    RpcServerConfig {
        max_fee,
        ..authenticated_config()
    }
}

fn make_authenticated_server_with_max_fee(max_fee: i64) -> RpcServer {
    let system = if Handle::try_current().is_ok() {
        crate::server::test_support::test_system(ProtocolSettings::default())
    } else {
        let rt = Runtime::new().expect("runtime");
        let system = rt.block_on(async {
            crate::server::test_support::test_system(ProtocolSettings::default())
        });
        drop(rt);
        system
    };
    let config = authenticated_config_with_max_fee(max_fee);
    RpcServer::new(system, config)
}

#[test]
fn wallet_signer_parser_applies_transfer_and_cancel_scopes() {
    let server = make_authenticated_server();
    let account = UInt160::zero();
    let address = wallet_helper::to_address(&account, server.system().settings().address_version);

    let transfer_signers = RpcServerWallet::parse_signers(
        &server,
        &Value::Array(vec![Value::String(address.clone())]),
    )
    .expect("transfer signers");
    assert_eq!(transfer_signers.len(), 1);
    assert_eq!(transfer_signers[0].account, account);
    assert_eq!(transfer_signers[0].scopes, WitnessScope::CALLED_BY_ENTRY);

    let cancel_entries = [Value::String(address)];
    let cancel_signers = RpcServerWallet::parse_signer_array(
        &server,
        &cancel_entries,
        "canceltransaction signers must be strings",
        WitnessScope::NONE,
    )
    .expect("cancel signers");
    assert_eq!(cancel_signers.len(), 1);
    assert_eq!(cancel_signers[0].account, account);
    assert_eq!(cancel_signers[0].scopes, WitnessScope::NONE);
}

#[test]
fn wallet_positive_amount_parser_preserves_error_messages() {
    let value = RpcServerWallet::parse_positive_amount(
        "1",
        8,
        || invalid_params("invalid amount"),
        || invalid_params("non-positive amount"),
    )
    .expect("positive amount");
    assert!(value.sign() > 0);

    let invalid: RpcError = RpcServerWallet::parse_positive_amount(
        "not-a-number",
        8,
        || invalid_params("invalid amount"),
        || invalid_params("non-positive amount"),
    )
    .expect_err("invalid amount")
    .into();
    assert_eq!(invalid.code(), RpcError::invalid_params().code());
    assert_eq!(invalid.data(), Some("invalid amount"));

    let non_positive: RpcError = RpcServerWallet::parse_positive_amount(
        "0",
        8,
        || invalid_params("invalid amount"),
        || invalid_params("non-positive amount"),
    )
    .expect_err("non-positive amount")
    .into();
    assert_eq!(non_positive.code(), RpcError::invalid_params().code());
    assert_eq!(non_positive.data(), Some("non-positive amount"));
}

fn mint_gas(
    store: &mut neo_storage::persistence::StoreCache,
    _settings: &ProtocolSettings,
    account: UInt160,
    amount: BigInt,
) {
    // Seeds the byte-exact NEP-17 account-state record the native
    // `balanceOf` reads; the legacy fixture invoked `GAS.Mint` through
    // an engine, which produces the same storage record.
    crate::server::test_support::seed_gas_balance(store, &account, amount);
}

fn build_signed_transaction_custom(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    script: Vec<u8>,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(1);
    tx.set_script(script);
    tx.set_signers(vec![Signer::new(
        keypair.script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn persist_transaction_record(store: &mut neo_storage::persistence::StoreCache, tx: &Transaction) {
    const PREFIX_TRANSACTION: u8 = 0x0b;

    // `Prefix_Transaction` value: the C# `TransactionState` interoperable
    // stack item serialized with `BinarySerializer`, matching the reader.
    let record = neo_native_contracts::LedgerContract::new()
        .serialize_persisted_transaction_state(0, VMState::NONE, tx)
        .expect("serialize TransactionState record");

    let mut key_bytes = Vec::with_capacity(1 + 32);
    key_bytes.push(PREFIX_TRANSACTION);
    key_bytes.extend_from_slice(&tx.hash().to_bytes());
    let key = StorageKey::new(LedgerContract::ID, key_bytes);
    store.add(key, StorageItem::from_bytes(record));
    store.commit();
}

#[path = "../rpc_server_wallet/cancel_transaction.rs"]
mod cancel_transaction;
#[path = "../rpc_server_wallet/lifecycle.rs"]
mod lifecycle;
#[path = "../rpc_server_wallet/network_fee.rs"]
mod network_fee;
#[path = "../rpc_server_wallet/no_params.rs"]
mod no_params;
#[path = "../rpc_server_wallet/send_from.rs"]
mod send_from;
#[path = "../rpc_server_wallet/send_many.rs"]
mod send_many;
#[path = "../rpc_server_wallet/send_to_address.rs"]
mod send_to_address;
