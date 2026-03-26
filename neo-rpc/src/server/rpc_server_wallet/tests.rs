use super::*;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_core::neo_io::BinaryWriter;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::conflicts::Conflicts;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::IVerifiable;
use neo_core::NeoSystem;
use neo_core::UInt256;
use neo_core::Witness;
use neo_crypto::Secp256r1Crypto;
use neo_vm::vm_state::VMState;
use num_bigint::BigInt;
use std::fs;
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
    let address = WalletHelper::to_address(&keypair.get_script_hash(), settings.address_version);
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
        NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
    } else {
        let rt = Runtime::new().expect("runtime");
        let system = rt.block_on(async {
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
        });
        drop(rt);
        system
    };
    let config = authenticated_config_with_max_fee(max_fee);
    RpcServer::new(system, config)
}

fn mint_gas(
    store: &mut neo_core::persistence::StoreCache,
    settings: &ProtocolSettings,
    account: UInt160,
    amount: BigInt,
) {
    let snapshot = Arc::new(store.data_cache().clone());
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        settings.clone(),
        400_000_000,
        None,
    )
    .expect("engine");

    let gas = GasToken::new();
    gas.mint(&mut engine, &account, &amount, false)
        .expect("mint");
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
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn persist_transaction_record(store: &mut neo_core::persistence::StoreCache, tx: &Transaction) {
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const RECORD_KIND_TRANSACTION: u8 = 0x01;

    let mut writer = BinaryWriter::new();
    writer
        .write_u8(RECORD_KIND_TRANSACTION)
        .expect("record kind");
    writer.write_u32(0).expect("block index");
    writer.write_u8(VMState::NONE as u8).expect("vm state");
    let tx_bytes = tx.to_bytes();
    writer.write_var_bytes(&tx_bytes).expect("tx bytes");

    let mut key_bytes = Vec::with_capacity(1 + 32);
    key_bytes.push(PREFIX_TRANSACTION);
    key_bytes.extend_from_slice(&tx.hash().to_bytes());
    let key = StorageKey::new(LedgerContract::ID, key_bytes);
    store.add(key, StorageItem::from_bytes(writer.to_bytes()));
    store.commit();
}

#[test]
fn signature_contract_pubkey_roundtrip() {
    let private_key = [1u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("pubkey");
    let script = ContractHelper::signature_redeem_script(&public_key);
    let recovered = signature_contract_pubkey(&script).expect("parse pubkey");
    assert_eq!(recovered, public_key);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_wallet_and_dump_priv_key_roundtrip() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case("openwallet"))
        .expect("openwallet handler");
    let dump_handler = handlers
        .iter()
        .find(|handler| {
            handler
                .descriptor()
                .name
                .eq_ignore_ascii_case("dumpprivkey")
        })
        .expect("dumpprivkey handler");
    let close_handler = handlers
        .iter()
        .find(|handler| {
            handler
                .descriptor()
                .name
                .eq_ignore_ascii_case("closewallet")
        })
        .expect("closewallet handler");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    let result = (open_handler.callback())(&server, &params).expect("open wallet");
    assert_eq!(result.as_bool(), Some(true));
    assert!(server.wallet().is_some());

    let params = [Value::String(address)];
    let result = (dump_handler.callback())(&server, &params).expect("dump priv key");
    assert_eq!(result.as_str().expect("wif"), keypair.to_wif());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));
    assert!(server.wallet().is_none());

    fs::remove_file(path).ok();
}

#[test]
fn close_wallet_returns_true_when_no_wallet_open() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let close_handler = find_handler(&handlers, "closewallet");

    assert!(server.wallet().is_none());
    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));
    assert!(server.wallet().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn open_wallet_rejects_invalid_password() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case("openwallet"))
        .expect("openwallet handler");

    let params = [
        Value::String(path.clone()),
        Value::String("wrong".to_string()),
    ];
    let err = (open_handler.callback())(&server, &params).expect_err("invalid password");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::wallet_not_supported().code());
    assert_eq!(rpc_error.data(), Some("Invalid password."));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn open_wallet_rejects_missing_file() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");

    let path = temp_wallet_path();
    let params = [
        Value::String(path.clone()),
        Value::String("password".to_string()),
    ];
    let err = (open_handler.callback())(&server, &params).expect_err("missing wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::wallet_not_found().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn open_wallet_rejects_invalid_wallet_format() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");

    let path = temp_wallet_path();
    fs::write(&path, "{}").expect("write invalid wallet");

    let params = [
        Value::String(path.clone()),
        Value::String("password".to_string()),
    ];
    let err = (open_handler.callback())(&server, &params).expect_err("invalid wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::wallet_not_supported().code());

    fs::remove_file(path).ok();
}

#[test]
fn get_new_address_adds_wallet_account() {
    let password = "rpc-pass";
    let rt = Runtime::new().expect("runtime");
    let (path, _keypair, _address) = rt.block_on(create_wallet_file(password));
    let system = rt.block_on(async {
        NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
    });
    drop(rt);
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let new_address_handler = find_handler(&handlers, "getnewaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    let result = (open_handler.callback())(&server, &params).expect("open wallet");
    assert_eq!(result.as_bool(), Some(true));

    let result = (new_address_handler.callback())(&server, &[]).expect("get new address");
    let new_address = result.as_str().expect("address");
    let wallet = server.wallet().expect("wallet");
    let accounts = wallet.get_accounts();
    assert!(accounts
        .iter()
        .any(|account| account.address() == new_address));

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_wallet_balance_reports_balance_field() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let balance_handler = find_handler(&handlers, "getwalletbalance");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = NeoToken::new().hash().to_string();
    let params = [Value::String(asset)];
    let result = (balance_handler.callback())(&server, &params).expect("get wallet balance");
    let obj = result.as_object().expect("balance object");
    assert!(obj.get("balance").is_some());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_wallet_balance_rejects_invalid_asset_id() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let balance_handler = find_handler(&handlers, "getwalletbalance");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [Value::String("NotAValidAssetID".to_string())];
    let err = (balance_handler.callback())(&server, &params).expect_err("invalid asset id");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_wallet_unclaimed_gas_returns_string() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let gas_handler = find_handler(&handlers, "getwalletunclaimedgas");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let result = (gas_handler.callback())(&server, &[]).expect("get wallet unclaimed gas");
    assert!(result.as_str().is_some());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[test]
fn import_priv_key_adds_account() {
    let password = "rpc-pass";
    let rt = Runtime::new().expect("runtime");
    let (path, _keypair, _address) = rt.block_on(create_wallet_file(password));
    let system = rt.block_on(async {
        NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
    });
    drop(rt);

    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let import_handler = find_handler(&handlers, "importprivkey");
    let list_handler = find_handler(&handlers, "listaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let new_key = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair");
    let wif = new_key.to_wif();
    let expected_address = WalletHelper::to_address(
        &new_key.get_script_hash(),
        ProtocolSettings::default().address_version,
    );

    let params = [Value::String(wif)];
    let result = (import_handler.callback())(&server, &params).expect("import privkey");
    let obj = result.as_object().expect("account json");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(expected_address.as_str())
    );
    assert_eq!(obj.get("haskey").and_then(Value::as_bool), Some(true));
    assert_eq!(obj.get("watchonly").and_then(Value::as_bool), Some(false));

    let result = (list_handler.callback())(&server, &[]).expect("listaddress");
    let accounts = result.as_array().expect("account list");
    assert!(accounts
        .iter()
        .filter_map(|entry| entry.as_object())
        .any(
            |entry| entry.get("address").and_then(Value::as_str) == Some(expected_address.as_str())
        ));

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn import_priv_key_rejects_invalid_wif() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let import_handler = find_handler(&handlers, "importprivkey");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [Value::String("ThisIsAnInvalidWIFString".to_string())];
    let err = (import_handler.callback())(&server, &params).expect_err("invalid wif");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn import_priv_key_returns_existing_account() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let import_handler = find_handler(&handlers, "importprivkey");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let wallet = server.wallet().expect("wallet");
    let existing = wallet
        .get_accounts()
        .into_iter()
        .find(|account| account.has_key())
        .expect("existing account");
    let existing_wif = existing.export_wif().expect("wif");
    let initial_count = wallet.get_accounts().len();

    let params = [Value::String(existing_wif)];
    let result = tokio::task::block_in_place(|| (import_handler.callback())(&server, &params))
        .expect("import existing");
    let obj = result.as_object().expect("account json");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(existing.address().as_str())
    );
    assert_eq!(obj.get("haskey").and_then(Value::as_bool), Some(true));
    assert_eq!(obj.get("watchonly").and_then(Value::as_bool), Some(false));
    if let Some(label) = existing.label() {
        assert_eq!(obj.get("label").and_then(Value::as_str), Some(label));
    } else {
        assert!(obj.get("label").is_some_and(Value::is_null));
    }

    let current_count = wallet.get_accounts().len();
    assert_eq!(current_count, initial_count);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn dump_priv_key_rejects_unknown_account() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let dump_handler = find_handler(&handlers, "dumpprivkey");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let other_key = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair");
    let other_address = WalletHelper::to_address(
        &other_key.get_script_hash(),
        ProtocolSettings::default().address_version,
    );
    let params = [Value::String(other_address)];
    let err = (dump_handler.callback())(&server, &params).expect_err("unknown account");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_account().code());
    let other_hash = other_key.get_script_hash().to_string();
    assert_eq!(rpc_error.data(), Some(other_hash.as_str()));

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn dump_priv_key_rejects_invalid_address_format() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let dump_handler = find_handler(&handlers, "dumpprivkey");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [Value::String("NotAValidAddress".to_string())];
    let err = (dump_handler.callback())(&server, &params).expect_err("invalid address");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[test]
fn cancel_transaction_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "canceltransaction");
    let txid = UInt256::from([0x11u8; 32]).to_string();
    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [
        Value::String(txid),
        Value::Array(vec![Value::String(address)]),
    ];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_txid() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String("invalid_txid".to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid txid");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_empty_signers() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x22u8; 32]).to_string()),
        Value::Array(Vec::new()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("empty signers");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::bad_request().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().context().store_snapshot_cache();
    mint_gas(
        &mut store,
        server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let txid = UInt256::from([0x33u8; 32]).to_string();
    let params = [
        Value::String(txid),
        Value::Array(vec![Value::String(address.clone())]),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let obj = result.as_object().expect("tx json");
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(address.as_str())
    );
    let signers = obj
        .get("signers")
        .and_then(Value::as_array)
        .expect("signers");
    let signer = signers[0].as_object().expect("signer");
    assert_eq!(signer.get("scopes").and_then(Value::as_str), Some("None"));
    let attributes = obj
        .get("attributes")
        .and_then(Value::as_array)
        .expect("attributes");
    assert_eq!(
        attributes[0].get("type").and_then(Value::as_str),
        Some("Conflicts")
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_signer_entry() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x66u8; 32]).to_string()),
        json!([{"account": "not-an-address"}]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid signer entry");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_confirmed_transaction() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let confirmed = build_signed_transaction_custom(
        server.system().settings(),
        &keypair,
        7,
        0,
        1,
        vec![OpCode::PUSH1 as u8],
    );
    let mut store = server.system().context().store_snapshot_cache();
    persist_transaction_record(&mut store, &confirmed);

    let params = [
        Value::String(confirmed.hash().to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("confirmed tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_extra_fee() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().context().store_snapshot_cache();
    mint_gas(
        &mut store,
        server.system().settings(),
        WalletHelper::to_script_hash(&address, server.system().settings().address_version)
            .expect("script hash"),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x44u8; 32]).to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("0".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid extra fee");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_wallet_fee_limit() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().context().store_snapshot_cache();
    mint_gas(
        &mut store,
        server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x77u8; 32]).to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("100".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("wallet fee limit");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::wallet_fee_limit().code());
    assert!(rpc_error.data().is_some());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_applies_extra_fee() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1_000_000_000);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().context().store_snapshot_cache();
    mint_gas(
        &mut store,
        server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let txid = UInt256::from([0x55u8; 32]);
    let conflict = TransactionAttribute::Conflicts(Conflicts::new(txid));
    let signers = vec![Signer::new(keypair.get_script_hash(), WitnessScope::NONE)];
    let snapshot = server.system().store_cache();
    let snapshot_arc = Arc::new(snapshot.data_cache().clone());
    let base_tx = Helper::make_transaction(
        server.wallet().expect("wallet").as_ref(),
        snapshot_arc.as_ref(),
        &[OpCode::RET as u8],
        Some(signers[0].account),
        Some(&signers),
        Some(std::slice::from_ref(&conflict)),
        server.system().settings(),
        None,
        server.settings().max_gas_invoke,
    )
    .expect("base cancel tx");
    let base_fee = base_tx.network_fee();

    let params = [
        Value::String(txid.to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("1".to_string()),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let net_fee = result
        .get("netfee")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("netfee");
    let expected_extra = 10_i64.pow(GasToken::new().decimals() as u32);
    assert_eq!(net_fee, base_fee + expected_extra);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_bumps_fee_for_mempool_conflict() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1_000_000_000);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().context().store_snapshot_cache();
    mint_gas(
        &mut store,
        server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let conflict_tx = build_signed_transaction_custom(
        server.system().settings(),
        &keypair,
        1,
        0,
        200_000_000,
        vec![OpCode::PUSH1 as u8],
    );
    let txid = conflict_tx.hash();
    let store_cache = server.system().store_cache();
    let verify = server.system().mempool().lock().try_add(
        conflict_tx.clone(),
        store_cache.data_cache(),
        server.system().settings(),
    );
    assert_eq!(verify, VerifyResult::Succeed);

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(txid.to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let net_fee = result
        .get("netfee")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("netfee");
    assert_eq!(net_fee, conflict_tx.network_fee() + 1);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}
#[test]
fn wallet_methods_require_open_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::new().hash().to_string();
    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let wif = keypair.to_wif();

    let cases = vec![
        ("dumpprivkey", vec![Value::String(address.clone())]),
        ("getnewaddress", vec![]),
        ("getwalletbalance", vec![Value::String(asset.clone())]),
        ("getwalletunclaimedgas", vec![]),
        ("importprivkey", vec![Value::String(wif.clone())]),
        ("listaddress", vec![]),
    ];

    for (name, params) in cases {
        let handler = find_handler(&handlers, name);
        let err = (handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::no_opened_wallet().code(),
            "{} should require a wallet",
            name
        );
    }
}

#[test]
fn send_from_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let send_handler = find_handler(&handlers, "sendfrom");
    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (send_handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_from_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let send_handler = find_handler(&handlers, "sendfrom");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        system.settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address.clone()),
        Value::String("1".to_string()),
    ];
    let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
        .expect("sendfrom");
    let obj = result.as_object().expect("tx json");
    assert_eq!(obj.len(), 12);
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(address.as_str())
    );

    let signers = obj
        .get("signers")
        .and_then(Value::as_array)
        .expect("signers");
    assert_eq!(signers.len(), 1);
    let signer = signers[0].as_object().expect("signer");
    let expected_account = keypair.get_script_hash().to_string();
    assert_eq!(
        signer.get("account").and_then(Value::as_str),
        Some(expected_account.as_str())
    );
    assert_eq!(
        signer.get("scopes").and_then(Value::as_str),
        Some("CalledByEntry")
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_from_returns_invalid_request_without_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let send_handler = find_handler(&handlers, "sendfrom");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address.clone()),
        Value::String("1".to_string()),
    ];
    let err = (send_handler.callback())(&server, &params).expect_err("insufficient funds");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_request().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[test]
fn send_to_address_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "sendtoaddress");
    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_invalid_asset_id() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [
        Value::String("NotAnAssetId".to_string()),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("invalid asset");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_invalid_to_address() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String("NotAnAddress".to_string()),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("invalid address");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_non_positive_amount() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    for amount in ["-1", "0"] {
        let params = [
            Value::String(asset.clone()),
            Value::String(address.clone()),
            Value::String(amount.to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid amount");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_reports_invalid_operation_on_insufficient_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address),
        Value::String("100000000000000000".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
    assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[test]
fn send_many_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "sendmany");
    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::new().hash().to_string();
    let outputs = json!([{
        "asset": asset,
        "value": "1",
        "address": address.clone()
    }]);
    let params = [Value::String(address), outputs];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_invalid_from() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let outputs = json!([{
        "asset": asset,
        "value": "1",
        "address": WalletHelper::to_address(
            &UInt160::zero(),
            server.system().settings().address_version,
        )
    }]);
    let params = [Value::String("NotAnAddress".to_string()), outputs];

    let err = (handler.callback())(&server, &params).expect_err("invalid from");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_empty_outputs() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [Value::String(address), Value::Array(vec![])];
    let err = (handler.callback())(&server, &params).expect_err("empty outputs");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Argument 'to' can't be empty"),
        "unexpected error message: {:?}",
        rpc_error.data()
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_invalid_outputs_type() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(address),
        Value::String("not-an-array".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid outputs");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid 'to' parameter"),
        "unexpected error message: {:?}",
        rpc_error.data()
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_non_positive_amount() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset_id = GasToken::new().hash();
    let asset = asset_id.to_string();
    for amount in ["-1", "0"] {
        let outputs = json!([{
            "asset": asset.clone(),
            "value": amount,
            "address": address.clone()
        }]);
        let params = [Value::String(address.clone()), outputs];
        let err = (handler.callback())(&server, &params).expect_err("invalid amount");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .unwrap_or_default()
                .contains(&format!("Amount of '{}' can't be negative.", asset_id)),
            "unexpected error message: {:?}",
            rpc_error.data()
        );
    }

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        system.settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::new().hash().to_string();
    let outputs = json!([{
        "asset": asset,
        "value": "1",
        "address": address.clone()
    }]);
    let params = [Value::String(address.clone()), outputs];
    let result =
        tokio::task::block_in_place(|| (handler.callback())(&server, &params)).expect("sendmany");
    let obj = result.as_object().expect("tx json");
    assert_eq!(obj.len(), 12);
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(address.as_str())
    );

    let signers = obj
        .get("signers")
        .and_then(Value::as_array)
        .expect("signers");
    assert_eq!(signers.len(), 1);
    let signer = signers[0].as_object().expect("signer");
    let expected_account = keypair.get_script_hash().to_string();
    assert_eq!(
        signer.get("account").and_then(Value::as_str),
        Some(expected_account.as_str())
    );
    assert_eq!(
        signer.get("scopes").and_then(Value::as_str),
        Some("CalledByEntry")
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_reports_invalid_operation_on_insufficient_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let outputs = json!([{
        "asset": GasToken::new().hash().to_string(),
        "value": "100000000000000000",
        "address": address.clone()
    }]);
    let params = [Value::String(address), outputs];
    let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
    assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[test]
fn calculate_network_fee_requires_payload() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let err = (handler.callback())(&server, &[]).expect_err("missing payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[test]
fn await_wallet_future_supports_current_thread_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let result = runtime.block_on(async {
        RpcServerWallet::await_wallet_future(Box::pin(async { Ok::<i32, WalletError>(7) }))
    });

    assert_eq!(result.expect("await_wallet_future result"), 7);
}

#[test]
fn calculate_network_fee_returns_network_fee() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let settings = ProtocolSettings::default();
    let keypair = KeyPair::from_private_key(&[0x66u8; 32]).expect("keypair");
    let tx =
        build_signed_transaction_custom(&settings, &keypair, 1, 0, 0, vec![OpCode::PUSH1 as u8]);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());

    let params = [Value::String(payload)];
    let result = (handler.callback())(&server, &params).expect("network fee");
    let obj = result.as_object().expect("network fee object");
    let fee = obj
        .get("networkfee")
        .and_then(Value::as_str)
        .expect("network fee");
    assert!(fee.parse::<i64>().is_ok());
}

#[test]
fn calculate_network_fee_rejects_invalid_payload() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");
    let params = [Value::String("invalid_base64".to_string())];

    let err = (handler.callback())(&server, &params).expect_err("invalid payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[test]
fn calculate_network_fee_rejects_invalid_transaction_bytes() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let payload = BASE64_STANDARD.encode([0x01u8, 0x02, 0x03]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}
