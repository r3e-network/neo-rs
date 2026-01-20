use super::*;
use crate::server::rcp_server_settings::RpcServerConfig;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::transaction::apply_tracked_items;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::smart_contract::iterators::{IteratorInterop, StorageIterator};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_core::smart_contract::native::{ContractManagement, NativeContract, NeoToken};
use neo_core::smart_contract::{
    ApplicationEngine, Contract, ContractParameterType, ContractState, FindOptions, IInteroperable,
    NefFile, StorageItem, StorageKey, TriggerType,
};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::wallet::{Wallet, WalletError, WalletResult};
use neo_core::wallets::{KeyPair, StandardWalletAccount, WalletAccount};
use neo_core::{NeoSystem, ProtocolSettings, UInt160, WitnessScope};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::op_code::OpCode;
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use num_bigint::BigInt;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::server::session::Session;

fn find_handler<'a>(
    handlers: &'a [crate::server::rpc_server::RpcHandler],
    name: &str,
) -> &'a crate::server::rpc_server::RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

fn make_server(config: RpcServerConfig) -> crate::server::rpc_server::RpcServer {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    crate::server::rpc_server::RpcServer::new(system, config)
}

struct TestWallet {
    name: String,
    account: Arc<dyn WalletAccount>,
}

#[async_trait::async_trait]
impl Wallet for TestWallet {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> Option<&str> {
        None
    }

    fn version(&self) -> &neo_core::wallets::Version {
        static VERSION: neo_core::wallets::Version = neo_core::wallets::Version::new(1, 0, 0);
        &VERSION
    }

    async fn change_password(&self, _old: &str, _new: &str) -> WalletResult<bool> {
        Err(WalletError::Other("not supported".to_string()))
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        &self.account.script_hash() == script_hash
    }

    async fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other("not supported".to_string()))
    }

    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        if &self.account.script_hash() == script_hash {
            Some(Arc::clone(&self.account))
        } else {
            None
        }
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        vec![Arc::clone(&self.account)]
    }

    async fn get_available_balance(&self, _asset_id: &neo_core::UInt256) -> WalletResult<i64> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn import_nep2(
        &self,
        _nep2: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn sign(&self, _data: &[u8], _script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn sign_transaction(&self, _transaction: &mut neo_core::Transaction) -> WalletResult<()> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn unlock(&self, _password: &str) -> WalletResult<bool> {
        Err(WalletError::Other("not supported".to_string()))
    }

    fn lock(&self) {}

    async fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(false)
    }

    async fn save(&self) -> WalletResult<()> {
        Err(WalletError::Other("not supported".to_string()))
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        Some(Arc::clone(&self.account))
    }

    async fn set_default_account(&self, _script_hash: &UInt160) -> WalletResult<()> {
        Err(WalletError::Other("not supported".to_string()))
    }
}

fn signature_contract_for_keypair(key_pair: &KeyPair) -> Contract {
    let script = ContractHelper::signature_redeem_script(&key_pair.compressed_public_key());
    Contract::create(vec![ContractParameterType::Signature], script)
}

fn fund_gas(system: &Arc<NeoSystem>, account: UInt160, amount: i64) {
    let mut store = system.context().store_snapshot_cache();
    let gas_id = neo_core::smart_contract::native::NativeRegistry::new()
        .get_by_name("GasToken")
        .expect("gas token")
        .id();
    let key = StorageKey::create_with_uint160(
        gas_id,
        neo_core::smart_contract::native::fungible_token::PREFIX_ACCOUNT,
        &account,
    );
    store.add(key, StorageItem::from_bigint(BigInt::from(amount)));
    store.commit();
}

fn deploy_verify_contract(system: &Arc<NeoSystem>) -> UInt160 {
    let mut store_cache = system.context().store_snapshot_cache();
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut builder = neo_vm::script_builder::ScriptBuilder::new();
    builder.emit_push_bool(true);
    builder.emit_opcode(OpCode::RET);
    let nef = NefFile::new("test".to_string(), builder.to_array());

    let verify_method = ContractMethodDescriptor::new(
        "verify".to_string(),
        Vec::<ContractParameterDefinition>::new(),
        ContractParameterType::Boolean,
        0,
        false,
    )
    .expect("verify method");

    let mut manifest = ContractManifest::new("VerifyContract".to_string());
    manifest.abi = ContractAbi::new(vec![verify_method], Vec::new());
    let manifest_json = manifest.to_json().expect("manifest json");
    let manifest_bytes = serde_json::to_vec(&manifest_json).expect("manifest bytes");

    let key_pair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
    let sender = key_pair.get_script_hash();
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)]);
    tx.add_witness(Witness::new());

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx)),
        Arc::clone(&snapshot),
        None,
        system.settings().clone(),
        50_000_000_000,
        None,
    )
    .expect("engine");

    let contract_bytes = engine
        .call_native_contract(
            ContractManagement::new().hash(),
            "deploy",
            &[nef.to_bytes(), manifest_bytes, Vec::new()],
        )
        .expect("deploy");

    let item =
        BinarySerializer::deserialize(&contract_bytes, &ExecutionEngineLimits::default(), None)
            .expect("contract stack item");
    let mut contract = ContractState::default();
    contract.from_stack_item(item);

    let tracked = engine.snapshot_cache().tracked_items();
    apply_tracked_items(&mut store_cache, tracked);
    store_cache.commit();

    contract.hash
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_returns_fault_state_in_result() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = vec![OpCode::ABORT as u8];
    let params = [Value::String(BASE64_STANDARD.encode(script))];
    let result = (invokescript.callback())(&server, &params).expect("invoke result");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state field");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception field");
    assert!(
        exception.contains("ABORT is executed"),
        "expected ABORT message, got {exception}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_returns_unknown_contract_for_missing_contract() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let unknown = UInt160::zero().to_string();
    let params = [Value::String(unknown)];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("should error");
    assert_eq!(err.code(), -102);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_rejects_invalid_hash() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [Value::String("invalid_script_hash".to_string())];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("invalid hash");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_returns_true_for_deployed_contract() {
    let server = make_server(RpcServerConfig::default());
    let contract_hash = deploy_verify_contract(&server.system());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [Value::String(contract_hash.to_string())];
    let result = (invokecontractverify.callback())(&server, &params).expect("invoke verify");
    assert_eq!(result.get("state").and_then(Value::as_str), Some("HALT"));

    let stack = result
        .get("stack")
        .and_then(Value::as_array)
        .expect("stack");
    let first = stack.first().expect("stack item");
    assert_eq!(first.get("type").and_then(Value::as_str), Some("Boolean"));
    assert_eq!(first.get("value").and_then(Value::as_bool), Some(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_rejects_missing_verify_overload() {
    let server = make_server(RpcServerConfig::default());
    let contract_hash = deploy_verify_contract(&server.system());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [
        Value::String(contract_hash.to_string()),
        json!([{"type": "Integer", "value": "0"}]),
    ];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("missing overload");
    assert_eq!(err.code(), -512);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_total_supply_matches_csharp() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let neo_hash = NeoToken::new().hash().to_string();
    let params = [
        Value::String(neo_hash),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke totalSupply");

    let script = result
        .get("script")
        .and_then(|value| value.as_str())
        .expect("script");
    assert_eq!(
        script,
        "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS"
    );

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Integer"));
    assert_eq!(
        first.get("value").and_then(|v| v.as_str()),
        Some("100000000")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_symbol_returns_byte_string() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let neo_hash = NeoToken::new().hash().to_string();
    let params = [
        Value::String(neo_hash),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke symbol");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(
        first.get("type").and_then(|v| v.as_str()),
        Some("ByteString")
    );
    assert_eq!(first.get("value").and_then(|v| v.as_str()), Some("TkVP"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_total_supply_matches_csharp() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS";
    let params = [Value::String(script.to_string())];
    let result = (invokescript.callback())(&server, &params).expect("invoke script");

    let echoed = result
        .get("script")
        .and_then(|value| value.as_str())
        .expect("script");
    assert_eq!(echoed, script);

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Integer"));
    assert_eq!(
        first.get("value").and_then(|v| v.as_str()),
        Some("100000000")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_transfer_returns_false() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "CxEMFPlu76Cuc+bgteStE4ozsOWTNUdrDBQtYNweHko3YcnMFOes3ceblcI/lRTAHwwIdHJhbnNmZXIMFPVj6kC8KD1NDgXEjqMFs/Kgc0DvQWJ9W1I=";
    let params = [Value::String(script.to_string())];
    let result = (invokescript.callback())(&server, &params).expect("invoke transfer script");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Boolean"));
    assert_eq!(first.get("value").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_with_diagnostics_includes_invoked_contract() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS";
    let params = [
        Value::String(script.to_string()),
        Value::Array(Vec::new()),
        Value::Bool(true),
    ];
    let result = (invokescript.callback())(&server, &params).expect("invoke script");

    let diagnostics = result
        .get("diagnostics")
        .and_then(Value::as_object)
        .expect("diagnostics");
    let invoked = diagnostics
        .get("invokedcontracts")
        .expect("invokedcontracts");

    fn collect_hashes(node: &Value, output: &mut Vec<String>) {
        let Some(obj) = node.as_object() else {
            return;
        };
        if let Some(hash) = obj.get("hash").and_then(Value::as_str) {
            output.push(hash.to_string());
        }
        if let Some(children) = obj.get("call").and_then(Value::as_array) {
            for child in children {
                collect_hashes(child, output);
            }
        }
    }

    let mut hashes = Vec::new();
    collect_hashes(invoked, &mut hashes);
    assert!(hashes.contains(&NeoToken::new().hash().to_string()));

    let storage_changes = diagnostics
        .get("storagechanges")
        .and_then(Value::as_array)
        .expect("storagechanges");
    assert!(!storage_changes.is_empty());
    let first_change = storage_changes
        .first()
        .and_then(Value::as_object)
        .expect("storage change object");
    assert!(first_change.contains_key("state"));
    assert!(first_change.contains_key("key"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_rejects_invalid_base64() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let params = [Value::String("not-base64".to_string())];
    let err = (invokescript.callback())(&server, &params).expect_err("invalid base64");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_faults_when_gas_limit_exceeded() {
    let config = RpcServerConfig {
        max_gas_invoke: 1_000_000,
        ..Default::default()
    };
    let max_gas = config.max_gas_invoke;
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let mut builder = neo_vm::script_builder::ScriptBuilder::new();
    builder.emit_jump(OpCode::JMP_L, 0).expect("jump loop");
    let script = builder.to_array();

    let params = [Value::String(BASE64_STANDARD.encode(script))];
    let result = (invokescript.callback())(&server, &params).expect("invoke loop");

    let state = result.get("state").and_then(Value::as_str).expect("state");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        exception.contains("Insufficient GAS"),
        "expected insufficient GAS error, got: {exception}"
    );

    let gas_consumed = result
        .get("gasconsumed")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("gasconsumed");
    assert!(gas_consumed >= max_gas);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_script_hash() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String("0x1234".to_string()),
        Value::String("symbol".to_string()),
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid hash");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_signer_scope() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
        "signer": {
            "account": UInt160::zero().to_string(),
            "scopes": "InvalidScopeValue"
        }
    }]);
    let params = [
        Value::String(UInt160::zero().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid scopes");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_signer_account() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
        "signer": {
            "account": "NotAValidHash160",
            "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid account");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_witness_invocation() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
        "signer": {
            "account": UInt160::zero().to_string(),
            "scopes": "CalledByEntry"
        },
        "witness": {
            "invocation": "!@#$",
            "verification": BASE64_STANDARD.encode([0x01])
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid invocation");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_witness_verification() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
        "signer": {
            "account": UInt160::zero().to_string(),
            "scopes": "CalledByEntry"
        },
        "witness": {
            "invocation": BASE64_STANDARD.encode([0x01]),
            "verification": "!@#$"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid verification");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_contract_parameter() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String(UInt160::zero().to_string()),
        Value::String("transfer".to_string()),
        json!([
            {"type": "Integer", "value": "NotAnInteger"}
        ]),
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid parameter");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_returns_fault_state_for_missing_method() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("nonExistentMethod".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");

    let state = result.get("state").and_then(Value::as_str).expect("state");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception");
    assert!(exception.contains("doesn't exist"));
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_sessions_disabled() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let err = (traverse.callback())(&server, &[]).expect_err("sessions disabled");
    assert_eq!(err.code(), -601);
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_session_rejects_sessions_disabled() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let terminate = find_handler(&handlers, "terminatesession");

    let err = (terminate.callback())(&server, &[]).expect_err("sessions disabled");
    assert_eq!(err.code(), -601);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_unknown_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let params = [
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(1)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("unknown session");
    assert_eq!(err.code(), -107);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_expired_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        session_expiration_time: 0,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let session = Session::new(
        server.system(),
        vec![OpCode::RET as u8],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");
    let session_id = server.store_session(session);

    let params = [
        Value::String(session_id.to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(1)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("expired session");
    assert_eq!(err.code(), -107);
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_session_returns_false_for_unknown_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let terminate = find_handler(&handlers, "terminatesession");

    let params = [Value::String(uuid::Uuid::new_v4().to_string())];
    let result = (terminate.callback())(&server, &params).expect("unknown session");
    assert_eq!(result, Value::Bool(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_count_limit_exceeded() {
    let config = RpcServerConfig {
        session_enabled: true,
        max_iterator_result_items: 1,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let params = [
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(2)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("count limit");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_returns_items_and_can_terminate_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        max_iterator_result_items: 10,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");
    let terminate = find_handler(&handlers, "terminatesession");

    let session = Session::new(
        server.system(),
        vec![OpCode::RET as u8],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let entries = vec![
        (
            StorageKey::new(1, vec![0x01]),
            StorageItem::from_bytes(vec![0xaa]),
        ),
        (
            StorageKey::new(1, vec![0x02]),
            StorageItem::from_bytes(vec![0xbb]),
        ),
    ];
    let iterator = StorageIterator::new(entries, 0, FindOptions::None);
    let iterator_id = {
        let mut engine = session.engine_mut();
        engine
            .store_storage_iterator(iterator)
            .expect("store iterator")
    };
    let interop = Arc::new(IteratorInterop::new(iterator_id)) as Arc<dyn VmInteropInterface>;
    let iterator_uuid = session
        .register_iterator_interface(&interop)
        .expect("iterator uuid");

    let session_id = server.store_session(session);
    let params = [
        Value::String(session_id.to_string()),
        Value::String(iterator_uuid.to_string()),
        Value::Number(serde_json::Number::from(10)),
    ];
    let result = (traverse.callback())(&server, &params).expect("traverse result");

    let items = result.as_array().expect("array");
    assert_eq!(items.len(), 2);
    for (index, expected_key, expected_value) in [
        (0usize, vec![0x01u8], vec![0xaau8]),
        (1usize, vec![0x02u8], vec![0xbbu8]),
    ] {
        let entry = items[index].as_object().expect("entry object");
        assert_eq!(entry.get("type").and_then(Value::as_str), Some("Struct"));
        let values = entry
            .get("value")
            .and_then(Value::as_array)
            .expect("value array");
        let key_bytes = values
            .first()
            .and_then(|item| item.get("value"))
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 key"))
            .expect("key bytes");
        let value_bytes = values
            .get(1)
            .and_then(|item| item.get("value"))
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 value"))
            .expect("value bytes");
        assert_eq!(key_bytes, expected_key);
        assert_eq!(value_bytes, expected_value);
    }

    let tail = (traverse.callback())(&server, &params).expect("traverse tail");
    assert!(tail.as_array().expect("array").is_empty());

    let terminate_result =
        (terminate.callback())(&server, &[Value::String(session_id.to_string())])
            .expect("terminate session");
    assert_eq!(terminate_result, Value::Bool(true));

    let err = (traverse.callback())(&server, &params).expect_err("unknown session");
    assert_eq!(err.code(), -107);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_wallet_returns_signed_tx() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account = StandardWalletAccount::new_with_key(key_pair, Some(contract), settings, None);
    let account_hash = account.script_hash();
    fund_gas(&server.system(), account_hash, 100_000_000);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let signers = json!([{
        "signer": {
            "account": account_hash.to_string(),
            "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");
    assert!(result.get("tx").is_some());
    assert!(result.get("pendingsignature").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_watch_only_wallet_returns_pending_signature() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account_hash = contract.script_hash();
    let account = StandardWalletAccount::new_watch_only(account_hash, Some(contract), settings);
    fund_gas(&server.system(), account_hash, 100_000_000);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let signers = json!([{
        "signer": {
            "account": account_hash.to_string(),
            "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");
    let pending = result
        .get("pendingsignature")
        .and_then(Value::as_object)
        .expect("pending signature");
    let items = pending
        .get("items")
        .and_then(Value::as_object)
        .expect("items");
    assert!(items.contains_key(&account_hash.to_string()));
    assert!(result.get("tx").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_missing_wallet_account_sets_exception() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account = StandardWalletAccount::new_with_key(key_pair, Some(contract), settings, None);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let missing_account = UInt160::from_bytes(&[0x42; 20]).expect("missing account hash");
    let missing_address =
        WalletHelper::to_address(&missing_account, server.system().settings().address_version);

    let signers = json!([{
        "signer": {
            "account": missing_account.to_string(),
            "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception");
    let expected = format!(
        "The smart contract or address {} ({}) is not found. If this is your wallet address and you want to sign a transaction with it, make sure you have opened this wallet.",
        missing_account.to_hex_string(),
        missing_address
    );
    assert_eq!(exception, expected);
    assert!(result.get("tx").is_none());
    assert!(result.get("pendingsignature").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_unclaimed_gas_rejects_invalid_address() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let get_unclaimed_gas = find_handler(&handlers, "getunclaimedgas");

    let params = [Value::String("not-an-address".to_string())];
    let err = (get_unclaimed_gas.callback())(&server, &params).expect_err("invalid address");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_unclaimed_gas_returns_address_string() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let get_unclaimed_gas = find_handler(&handlers, "getunclaimedgas");

    let address =
        WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [Value::String(address.clone())];
    let result = (get_unclaimed_gas.callback())(&server, &params).expect("unclaimed gas");

    let address_value = result
        .get("address")
        .and_then(Value::as_str)
        .expect("address");
    assert_eq!(address_value, address);

    let unclaimed = result
        .get("unclaimed")
        .and_then(Value::as_str)
        .expect("unclaimed");
    assert!(unclaimed.parse::<f64>().is_ok());
}
