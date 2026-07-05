use super::*;

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

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = rt
        .block_on(async { crate::server::test_support::test_system(ProtocolSettings::default()) });
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
    let accounts = wallet.accounts();
    assert!(
        accounts
            .iter()
            .any(|account| account.address() == new_address)
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_wallet_balance_reports_balance_field() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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

    let asset = NeoToken::script_hash().to_string();
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

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = rt
        .block_on(async { crate::server::test_support::test_system(ProtocolSettings::default()) });
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
    let expected_address = wallet_helper::to_address(
        &new_key.script_hash(),
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
    assert!(
        accounts
            .iter()
            .filter_map(|entry| entry.as_object())
            .any(|entry| entry.get("address").and_then(Value::as_str)
                == Some(expected_address.as_str()))
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn import_priv_key_rejects_invalid_wif() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
        .accounts()
        .into_iter()
        .find(|account| account.has_key())
        .expect("existing account");
    let existing_wif = existing.export_wif().expect("wif");
    let initial_count = wallet.accounts().len();

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

    let current_count = wallet.accounts().len();
    assert_eq!(current_count, initial_count);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn dump_priv_key_rejects_unknown_account() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let other_address = wallet_helper::to_address(
        &other_key.script_hash(),
        ProtocolSettings::default().address_version,
    );
    let params = [Value::String(other_address)];
    let err = (dump_handler.callback())(&server, &params).expect_err("unknown account");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_account().code());
    let other_hash = other_key.script_hash().to_string();
    assert_eq!(rpc_error.data(), Some(other_hash.as_str()));

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn dump_priv_key_rejects_invalid_address_format() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;

    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
