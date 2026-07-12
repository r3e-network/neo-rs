use super::*;

#[test]
fn wallet_methods_require_open_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
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
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let send_handler = find_handler(&handlers, "sendfrom");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &system.settings(),
        keypair.script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
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
    let expected_account = keypair.script_hash().to_string();
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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

    let asset = GasToken::script_hash().to_string();
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

// Fix 19: a wallet holding the member keys of a multi-sig contract must
// contribute each held member's signature to the ContractParametersContext,
// mirroring C# `Wallet.Sign` (Wallet.cs:700-719). Before the fix, `sign_and_relay`
// only signed a multi-sig signer with the multi-sig account's own key, so a
// 2-of-2 send-from could never complete and would fall back to a pending context.
#[tokio::test(flavor = "multi_thread")]
async fn send_from_multisig_collects_member_signatures() {
    use neo_execution::contract::Contract;
    let settings = Arc::new(ProtocolSettings::default());
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let send_handler = find_handler(&handlers, "sendfrom");

    // Build an in-memory NEP-6 wallet with two single-sig member accounts (each
    // holding its private key) plus a watch-only 2-of-2 multi-sig account.
    let wallet = Nep6Wallet::new(Some("multisig".to_string()), None, settings.clone());
    let member1 = KeyPair::from_private_key(&[0x21u8; 32]).expect("member1");
    let member2 = KeyPair::from_private_key(&[0x22u8; 32]).expect("member2");
    wallet
        .create_account(&[0x21u8; 32])
        .await
        .expect("member1 account");
    wallet
        .create_account(&[0x22u8; 32])
        .await
        .expect("member2 account");

    let points = vec![
        member1.public_key_point().expect("point1"),
        member2.public_key_point().expect("point2"),
    ];
    let multisig_contract = Contract::create_multi_sig_contract(2, &points);
    let multisig_hash = multisig_contract.script_hash();
    wallet
        .create_account_with_contract(multisig_contract, None)
        .await
        .expect("multisig account");

    server.set_wallet(Some(Arc::new(wallet)));

    // Fund the multi-sig account so `make_transaction` can build the transfer.
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &system.settings(),
        multisig_hash,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let multisig_address =
        wallet_helper::to_address(&multisig_hash, system.settings().address_version);
    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(multisig_address.clone()),
        Value::String(multisig_address.clone()),
        Value::String("1".to_string()),
    ];
    let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
        .expect("sendfrom multisig");
    let obj = result.as_object().expect("tx json");

    // A completed multi-sig signing returns the full 12-field transaction JSON,
    // not a pending ContractParametersContext (which would have "type"/"items").
    assert!(
        obj.get("hash").is_some(),
        "expected a completed transaction, got: {result}"
    );
    assert_eq!(obj.len(), 12, "expected full tx json, got: {result}");
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(multisig_address.as_str())
    );

    // The single witness must carry both member signatures (2 * PUSHDATA1 64B).
    let witnesses = obj
        .get("witnesses")
        .and_then(Value::as_array)
        .expect("witnesses");
    assert_eq!(witnesses.len(), 1, "one multi-sig witness expected");
    let invocation_b64 = witnesses[0]
        .as_object()
        .and_then(|w| w.get("invocation"))
        .and_then(Value::as_str)
        .expect("invocation script");
    let invocation = BASE64_STANDARD
        .decode(invocation_b64)
        .expect("decode invocation");
    // Each signature is emitted as PUSHDATA1 (0x0c) + len(0x40) + 64 bytes = 66.
    let sig_pushes = invocation
        .chunks(66)
        .filter(|c| c.len() == 66 && c[0] == 0x0c && c[1] == 0x40)
        .count();
    assert_eq!(
        sig_pushes, 2,
        "expected 2 member signatures in the multi-sig invocation script"
    );
}
