// Copyright (C) 2015-2025 The Neo Project.
//
// transaction_manager.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{Nep17Api, RpcClient, TransactionManagerFactory};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::persistence::DataCache;
use neo_core::smart_contract::native::GasToken;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{
    smart_contract::ContractParametersContext, Contract, ECPoint, IVerifiable, KeyPair,
    NativeContract, Signer, Transaction, TransactionAttribute, Witness,
};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use std::sync::Arc;

/// Sign item for transaction signing
#[derive(Clone)]
struct SignItem {
    /// The contract for signing
    contract: Contract,
    /// The key pairs for signing
    key_pairs: Vec<KeyPair>,
}

/// This class helps to create transaction with RPC API
/// Matches C# `TransactionManager`
pub struct TransactionManager {
    /// The RPC client instance
    _rpc_client: Arc<RpcClient>,

    /// The Transaction context to manage the witnesses
    context: ContractParametersContext,

    /// This container stores the keys for sign the transaction
    sign_store: Vec<SignItem>,

    /// The Transaction managed by this instance
    tx: Transaction,
}

impl TransactionManager {
    /// `TransactionManager` Constructor
    /// Matches C# constructor
    pub fn new(tx: Transaction, rpc_client: Arc<RpcClient>) -> Self {
        let snapshot = std::sync::Arc::new(neo_core::persistence::DataCache::new(true));
        let context = ContractParametersContext::new(
            snapshot,
            tx.clone(),
            rpc_client.protocol_settings.network,
        );

        Self {
            _rpc_client: rpc_client,
            context,
            sign_store: Vec::new(),
            tx,
        }
    }

    /// Get the managed transaction
    pub const fn tx(&self) -> &Transaction {
        &self.tx
    }

    /// Helper function for one-off `TransactionManager` creation
    /// Matches C# `MakeTransactionAsync`
    pub async fn make_transaction(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        signers: Option<Vec<Signer>>,
        _attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let factory = TransactionManagerFactory::new(rpc_client);
        factory
            .make_transaction(script, &signers.unwrap_or_default())
            .await
    }

    /// Helper function for one-off `TransactionManager` creation with system fee
    /// Matches C# `MakeTransactionAsync` with systemFee parameter
    pub async fn make_transaction_with_fee(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        system_fee: i64,
        signers: Option<Vec<Signer>>,
        attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let factory = TransactionManagerFactory::new(rpc_client);
        let mut manager = factory
            .make_transaction(script, &signers.unwrap_or_default())
            .await?;
        manager.tx.set_system_fee(system_fee);

        if let Some(attrs) = attributes {
            manager.tx.set_attributes(attrs);
        }

        Ok(manager)
    }

    /// Add Signature
    /// Matches C# `AddSignature`
    pub fn add_signature(
        &mut self,
        key: &KeyPair,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let public_point = key.get_public_key_point()?;
        let contract = Contract::create_signature_contract(public_point);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Add Multi-Signature
    /// Matches C# `AddMultiSig` with `KeyPair`
    pub fn add_multi_sig(
        &mut self,
        key: &KeyPair,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Add Multi-Signature with multiple keys
    /// Matches C# `AddMultiSig` with `KeyPair` array
    pub fn add_multi_sig_with_keys(
        &mut self,
        keys: Vec<KeyPair>,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);

        for key in keys {
            self.add_sign_item(contract.clone(), key)?;
        }

        Ok(self)
    }

    /// Add witness with contract
    /// Matches C# `AddWitness`
    pub fn add_witness(
        &mut self,
        contract: Contract,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        if !self.context.add_contract(contract) {
            return Err("AddWitness failed!".into());
        }
        Ok(self)
    }

    /// Add witness with script hash
    /// Matches C# `AddWitness` with `UInt160`.
    ///
    /// Note: Contract lookup requires an RPC call; use [`Self::add_witness_with_hash_async`].
    pub fn add_witness_with_hash(
        &mut self,
        script_hash: &UInt160,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = Contract::create_with_hash(*script_hash, Vec::new());
        self.add_witness(contract)
    }

    /// Adds a witness by resolving the contract over RPC (required for contract accounts).
    pub async fn add_witness_with_hash_async(
        &mut self,
        script_hash: &UInt160,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = self.get_contract_async(script_hash).await?;
        self.add_witness(contract)
    }

    /// Sign the transaction
    /// Matches C# `SignAsync`
    pub async fn sign(&mut self) -> Result<Transaction, Box<dyn std::error::Error>> {
        let script_hashes = self
            .tx
            .get_script_hashes_for_verifying(&DataCache::new(true));
        let mut witnesses = Vec::with_capacity(script_hashes.len());
        for hash in &script_hashes {
            let verification_script = self.get_verification_script(hash);
            witnesses.push(Witness::new_with_scripts(Vec::new(), verification_script));
        }
        self.tx.set_witnesses(witnesses);

        let network_fee = self._rpc_client.calculate_network_fee(&self.tx).await?;
        self.tx.set_network_fee(network_fee);
        self.tx.set_witnesses(Vec::new());

        let sender = self
            .tx
            .sender()
            .ok_or_else(|| "Sender not specified in transaction".to_string())?;
        let gas_balance = Nep17Api::new(self._rpc_client.clone())
            .balance_of(&GasToken::new().hash(), &sender)
            .await?;
        let required_fee = BigInt::from(self.tx.system_fee() + self.tx.network_fee());
        if gas_balance < required_fee {
            let address = WalletHelper::to_address(
                &sender,
                self._rpc_client.protocol_settings.address_version,
            );
            return Err(format!("Insufficient GAS in address: {address}").into());
        }

        let sign_data = get_sign_data_vec(&self.tx, self._rpc_client.protocol_settings.network)?;
        for item in &self.sign_store {
            for key in &item.key_pairs {
                let signature = key.sign(&sign_data)?;
                let public_key = key.get_public_key_point()?;
                let added = self
                    .context
                    .add_signature(item.contract.clone(), public_key, signature)
                    .map_err(|err| format!("AddSignature failed: {err}"))?;
                if !added {
                    return Err("AddSignature failed!".into());
                }
            }
        }

        if !self.context.completed() {
            return Err("Please add signature or witness first!".into());
        }

        let final_witnesses = self
            .context
            .get_witnesses()
            .ok_or_else(|| "No witnesses available; context incomplete".to_string())?;
        self.tx.set_witnesses(final_witnesses);

        Ok(self.tx.clone())
    }

    // Helper methods

    fn add_sign_item(
        &mut self,
        contract: Contract,
        key: KeyPair,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = contract.script_hash();
        let script_hashes = self
            .tx
            .get_script_hashes_for_verifying(&DataCache::new(true));
        if !script_hashes.contains(&hash) {
            return Err(format!("Add SignItem error: Mismatch ScriptHash ({hash})").into());
        }
        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|i| i.contract.script_hash() == hash)
        {
            let exists = item
                .key_pairs
                .iter()
                .any(|candidate| candidate.private_key() == key.private_key());
            if !exists {
                item.key_pairs.push(key);
            }
        } else {
            let key_pairs = vec![key];
            self.sign_store.push(SignItem {
                contract: contract.clone(),
                key_pairs,
            });
        }

        self.context.add_contract(contract);
        Ok(())
    }

    fn get_verification_script(&self, hash: &UInt160) -> Vec<u8> {
        for item in &self.sign_store {
            if item.contract.script_hash() == *hash {
                return item.contract.script.clone();
            }
        }
        Vec::new()
    }

    async fn get_contract_async(
        &self,
        script_hash: &UInt160,
    ) -> Result<Contract, Box<dyn std::error::Error>> {
        let state = self
            ._rpc_client
            .get_contract_state(&script_hash.to_string())
            .await?;

        let parameter_list = state
            .contract_state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("verify"))
            .map(|method| method.parameters.iter().map(|p| p.param_type).collect())
            .unwrap_or_default();

        Ok(Contract::create_with_hash(*script_hash, parameter_list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use neo_config::ProtocolSettings;
    use neo_json::{JArray, JObject, JToken};
    use neo_primitives::WitnessScope;
    use reqwest::Url;
    use std::net::TcpListener;

    fn localhost_binding_permitted() -> bool {
        TcpListener::bind("127.0.0.1:0").is_ok()
    }

    fn rpc_response(result: JToken) -> String {
        let mut response = JObject::new();
        response.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
        response.insert("id".to_string(), JToken::Number(1.0));
        response.insert("result".to_string(), result);
        JToken::Object(response).to_string()
    }

    fn invoke_result_payload(gas_consumed: i64, balance: &str) -> JObject {
        let mut result = JObject::new();
        result.insert("script".to_string(), JToken::String("AA==".to_string()));
        result.insert("state".to_string(), JToken::String("HALT".to_string()));
        result.insert(
            "gasconsumed".to_string(),
            JToken::String(gas_consumed.to_string()),
        );

        let mut stack_item = JObject::new();
        stack_item.insert("type".to_string(), JToken::String("Integer".to_string()));
        stack_item.insert("value".to_string(), JToken::String(balance.to_string()));
        let stack = JArray::from(vec![JToken::Object(stack_item)]);
        result.insert("stack".to_string(), JToken::Array(stack));
        result
    }

    fn mock_invokescript(server: &mut Server, response_body: &str) {
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body)
            .expect_at_least(1)
            .create();
    }

    fn mock_block_count(server: &mut Server, count: u32) {
        let response = rpc_response(JToken::Number(count as f64));
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(r#""method"\s*:\s*"getblockcount""#.into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response)
            .expect(1)
            .create();
    }

    fn mock_calculate_network_fee(server: &mut Server, fee: i64) {
        let mut result = JObject::new();
        result.insert("networkfee".to_string(), JToken::Number(fee as f64));
        let response = rpc_response(JToken::Object(result));
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(
                r#""method"\s*:\s*"calculatenetworkfee""#.into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response)
            .expect(1)
            .create();
    }

    fn mock_calculate_network_fee_with_hits(server: &mut Server, fee: i64, hits: usize) {
        let mut result = JObject::new();
        result.insert("networkfee".to_string(), JToken::Number(fee as f64));
        let response = rpc_response(JToken::Object(result));
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(
                r#""method"\s*:\s*"calculatenetworkfee""#.into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response)
            .expect(hits)
            .create();
    }

    #[tokio::test]
    async fn make_transaction_preserves_signer_scope() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(RpcClient::builder(url).build().unwrap());
        let key = KeyPair::from_private_key(&[1u8; 32]).expect("key");
        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

        let manager = TransactionManager::make_transaction(client, &[0x01], Some(signers), None)
            .await
            .expect("manager");

        assert_eq!(manager.tx().signers()[0].scopes(), WitnessScope::GLOBAL);
    }

    #[tokio::test]
    async fn sign_adds_signature_and_sets_fees() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee(&mut server, 100_000_000);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(
            RpcClient::builder(url)
                .protocol_settings(ProtocolSettings::default_settings())
                .build()
                .unwrap(),
        );
        let key = KeyPair::from_private_key(&[2u8; 32]).expect("key");
        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        manager.add_signature(&key).expect("add signature");
        let tx = manager.sign().await.expect("sign");

        assert_eq!(tx.network_fee(), 100_000_000);
        assert_eq!(tx.system_fee(), 100);
        assert_eq!(tx.witnesses().len(), 1);

        let invocation = tx.witnesses()[0].invocation_script();
        assert_eq!(invocation.len(), 66);
        let signature = &invocation[2..];
        let sign_data =
            get_sign_data_vec(&tx, client.protocol_settings.network).expect("sign data");
        assert!(key.verify(&sign_data, signature).expect("verify signature"));
    }

    #[tokio::test]
    async fn sign_rejects_mismatched_key() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee(&mut server, 100_000_000);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(RpcClient::builder(url).build().unwrap());
        let key = KeyPair::from_private_key(&[3u8; 32]).expect("key");
        let wrong_key = KeyPair::from_private_key(&[4u8; 32]).expect("wrong key");
        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        let err = manager
            .add_signature(&wrong_key)
            .err()
            .expect("mismatched key");
        assert!(err.to_string().contains("Mismatch ScriptHash"));
    }

    #[tokio::test]
    async fn sign_rejects_duplicate_signature() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee_with_hits(&mut server, 100_000_000, 2);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(
            RpcClient::builder(url)
                .protocol_settings(ProtocolSettings::default_settings())
                .build()
                .unwrap(),
        );
        let key = KeyPair::from_private_key(&[9u8; 32]).expect("key");
        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        manager.add_signature(&key).expect("add signature");
        manager.sign().await.expect("sign");

        manager.add_signature(&key).expect("add signature again");
        let err = manager.sign().await.expect_err("duplicate signature");
        assert!(err.to_string().contains("AddSignature failed"));
        assert!(manager.tx().witnesses().is_empty());
    }

    #[tokio::test]
    async fn sign_rejects_insufficient_gas() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee(&mut server, 100_000_000);
        let invoke_result = invoke_result_payload(100, "1");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(
            RpcClient::builder(url)
                .protocol_settings(ProtocolSettings::default_settings())
                .build()
                .unwrap(),
        );
        let key = KeyPair::from_private_key(&[8u8; 32]).expect("key");
        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        manager.add_signature(&key).expect("add signature");

        let err = manager.sign().await.expect_err("insufficient gas");
        assert!(err.to_string().contains("Insufficient GAS"));
    }

    #[tokio::test]
    async fn sign_multi_sig_contract() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee(&mut server, 100_000_000);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(RpcClient::builder(url).build().unwrap());
        let key_a = KeyPair::from_private_key(&[5u8; 32]).expect("key a");
        let key_b = KeyPair::from_private_key(&[6u8; 32]).expect("key b");
        let pub_a = key_a.get_public_key_point().expect("pub a");
        let pub_b = key_b.get_public_key_point().expect("pub b");
        let contract = Contract::create_multi_sig_contract(2, &[pub_a.clone(), pub_b.clone()]);
        let signers = vec![Signer::new(contract.script_hash(), WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        manager
            .add_multi_sig(&key_a, 2, vec![pub_a.clone(), pub_b.clone()])
            .expect("add multisig a");
        manager
            .add_multi_sig(&key_b, 2, vec![pub_a, pub_b])
            .expect("add multisig b");
        let tx = manager.sign().await.expect("sign");
        assert_eq!(tx.witnesses().len(), 1);
    }

    #[tokio::test]
    async fn add_witness_by_hash_adds_second_witness() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        mock_block_count(&mut server, 100);
        mock_calculate_network_fee(&mut server, 100_000_000);
        let invoke_result = invoke_result_payload(100, "10000000000000000");
        let response_body = rpc_response(JToken::Object(invoke_result));
        mock_invokescript(&mut server, &response_body);

        let url = Url::parse(&server.url()).unwrap();
        let client = Arc::new(RpcClient::builder(url).build().unwrap());
        let key = KeyPair::from_private_key(&[7u8; 32]).expect("key");
        let sender = key.get_script_hash();
        let signers = vec![
            Signer::new(sender, WitnessScope::GLOBAL),
            Signer::new(UInt160::zero(), WitnessScope::GLOBAL),
        ];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        manager
            .add_witness_with_hash(&UInt160::zero())
            .expect("add witness");
        manager.add_signature(&key).expect("add signature");
        let tx = manager.sign().await.expect("sign");
        assert_eq!(tx.witnesses().len(), 2);
        assert_eq!(tx.witnesses()[0].verification_script().len(), 40);
        assert_eq!(tx.witnesses()[0].invocation_script().len(), 66);
    }
}
