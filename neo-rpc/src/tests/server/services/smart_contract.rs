use super::*;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_execution::contract::Contract;
use neo_execution::helper::Helper as ContractHelper;
use neo_execution::iterators::{IteratorInterop, StorageIterator};
use neo_execution::{ApplicationEngine, ContractState, TriggerType};
use neo_manifest::NefFile;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_native_contracts::{ContractManagement, CryptoLib, NeoToken};
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{CallFlags, ContractParameterType, FindOptions};
use neo_primitives::{UInt160, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_storage::{StorageItem, StorageKey};
use neo_wallets::wallet::{Wallet, WalletError, WalletResult};
use neo_wallets::wallet_helper::WalletAddress as address_helper;
use neo_wallets::{KeyPair, StandardWalletAccount, WalletAccount};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use neo_vm_rs::{ExecutionEngineLimits, OpCode};
use num_bigint::BigInt;
use serde_json::{Value, json};
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    crate::server::rpc_server::RpcServer::new(system, config)
}

fn assert_invalid_params_data(err: &crate::server::rpc_exception::RpcException, data: &str) {
    assert_eq!(err.code(), -32602);
    assert_eq!(err.message(), "Invalid params");
    assert_eq!(err.data(), Some(data));
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

    fn version(&self) -> &neo_wallets::Version {
        static VERSION: neo_wallets::Version = neo_wallets::Version::new(1, 0, 0);
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

    fn account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        if &self.account.script_hash() == script_hash {
            Some(Arc::clone(&self.account))
        } else {
            None
        }
    }

    fn accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        vec![Arc::clone(&self.account)]
    }

    async fn available_balance(
        &self,
        _asset_id: &neo_primitives::UInt256,
    ) -> WalletResult<i64> {
        Err(WalletError::Other("not supported".to_string()))
    }

    async fn unclaimed_gas(&self) -> WalletResult<i64> {
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

    async fn sign_transaction(
        &self,
        _transaction: &mut neo_payloads::Transaction,
    ) -> WalletResult<()> {
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

    fn default_account(&self) -> Option<Arc<dyn WalletAccount>> {
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

fn fund_gas(system: &Arc<crate::server::NodeContext>, account: UInt160, amount: i64) {
    let mut store = system.store_cache();
    crate::server::test_support::seed_gas_balance(&mut store, &account, BigInt::from(amount));
    store.commit();
}

fn deploy_verify_contract(system: &Arc<crate::server::NodeContext>) -> UInt160 {
    let mut store_cache = system.store_cache();
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
    let sender = key_pair.script_hash();
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)]);
    tx.add_witness(Witness::new());

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx)),
        Arc::clone(&snapshot),
        None,
        system.settings().as_ref().clone(),
        50_000_000_000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("entry script loads");

    let contract_bytes = engine
        .call_native_contract(
            ContractManagement::new().hash(),
            "deploy",
            // Two-argument overload: the optional `data` argument is
            // `StackItem::Null` exactly as in C# `Deploy(nef, manifest)`.
            &[nef.to_bytes(), manifest_bytes],
        )
        .expect("deploy");

    let item =
        BinarySerializer::deserialize(&contract_bytes, &ExecutionEngineLimits::default(), None)
            .expect("contract stack item");
    let mut contract = ContractState::default();
    let sv = neo_vm_rs::StackValue::try_from(item).expect("stack item to stack value");
    let _ = contract.from_stack_value(sv);

    let tracked = engine.snapshot_cache().tracked_items();
    store_cache.apply_tracked_items(tracked);
    store_cache.commit();

    contract.hash
}

#[path = "../smart_contract/contract_verify.rs"]
mod contract_verify;
#[path = "../smart_contract/script_and_function_invocation.rs"]
mod script_and_function_invocation;
#[path = "../smart_contract/sessions.rs"]
mod sessions;
#[path = "../smart_contract/validation_errors.rs"]
mod validation_errors;
#[path = "../smart_contract/wallet_and_gas.rs"]
mod wallet_and_gas;
