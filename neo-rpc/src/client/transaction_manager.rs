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

use crate::{Nep17Api, RpcClient, RpcError, TransactionManagerFactory};
use neo_protocol::signing_data::get_sign_data_vec;
use neo_wallets::helper::Helper as WalletHelper;
use neo_wallets::KeyPair;
use neo_ledger::VerifiableExt;
use neo_native_contracts::native::{GasToken, NativeContract};
use neo_primitives::UInt160;
use neo_protocol::payloads::{Signer, Transaction, TransactionAttribute};
use neo_protocol::witness::Witness;
use neo_smart_contract::contract::Contract;
use neo_storage::persistence::DataCache;
use neo_crypto::ECPoint;
use neo_vm::ScriptBuilder;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::sync::Arc;

/// Represents the context item for a single script hash during transaction signing.
///
/// Manages signature collection and witness construction for one signer/contract.
/// Based on C# `Neo.SmartContract.ContractParametersContext.ContextItem`.
#[derive(Clone, Debug)]
struct ContextItem {
    /// The contract script (verification script)
    script: Vec<u8>,
    /// Expected parameter count (signatures needed)
    parameter_count: usize,
    /// Collected signatures, indexed by public key
    signatures: HashMap<ECPoint, Vec<u8>>,
}

impl ContextItem {
    /// Creates a new context item for a contract.
    fn new(script: Vec<u8>, parameter_count: usize) -> Self {
        Self {
            script,
            parameter_count,
            signatures: HashMap::new(),
        }
    }

    /// Adds a signature for a given public key.
    /// Returns `true` if the signature was newly added, `false` if it already existed.
    fn add_signature(&mut self, public_key: ECPoint, signature: Vec<u8>) -> bool {
        if self.signatures.contains_key(&public_key) {
            false
        } else {
            self.signatures.insert(public_key, signature);
            true
        }
    }

    /// Checks if this context item has collected all required signatures.
    fn is_completed(&self) -> bool {
        self.signatures.len() >= self.parameter_count
    }

    /// Builds the witness from collected signatures.
    ///
    /// For single-sig contracts: invocation script contains one signature.
    /// For multi-sig contracts: invocation script contains M signatures in order.
    fn build_witness(&self, public_keys: &[ECPoint]) -> Witness {
        let invocation_script = self.build_invocation_script(public_keys);
        Witness::new_with_scripts(invocation_script, self.script.clone())
    }

    /// Builds the invocation script from collected signatures.
    ///
    /// Format: PUSHDATA1 <sig1> [PUSHDATA1 <sig2> ...]
    /// Signatures are ordered by the public key order in the verification script.
    fn build_invocation_script(&self, public_keys: &[ECPoint]) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();

        // For single signature contracts
        if self.parameter_count == 1 {
            if let Some(sig) = self.signatures.values().next() {
                builder.emit_push(sig);
            }
            return builder.to_array();
        }

        // For multi-signature contracts: emit signatures in public key order
        let mut ordered_sigs = Vec::new();
        for pubkey in public_keys {
            if let Some(sig) = self.signatures.get(pubkey) {
                ordered_sigs.push(sig);
                if ordered_sigs.len() >= self.parameter_count {
                    break;
                }
            }
        }

        for sig in ordered_sigs {
            builder.emit_push(sig);
        }

        builder.to_array()
    }
}

/// Contract parameters context for managing transaction witness construction.
///
/// Tracks signature collection across multiple signers and builds complete witnesses
/// when all required signatures are gathered.
///
/// Based on C# `Neo.SmartContract.ContractParametersContext`.
#[derive(Clone)]
struct ContractParametersContext {
    /// The transaction being signed
    tx: Transaction,
    /// Context items per script hash
    items: HashMap<UInt160, ContextItem>,
    /// Network magic number
    network: u32,
}

impl ContractParametersContext {
    /// Creates a new contract parameters context for a transaction.
    ///
    /// # Arguments
    /// * `snapshot` - Blockchain snapshot (currently unused, matches C# signature)
    /// * `tx` - The transaction to sign
    /// * `network` - Network magic number for signature domain separation
    fn new(
        _snapshot: Arc<DataCache>,
        tx: Transaction,
        network: u32,
    ) -> Self {
        Self {
            tx,
            items: HashMap::new(),
            network,
        }
    }

    /// Adds a signature for a specific contract and public key.
    ///
    /// # Returns
    /// * `Ok(true)` - Signature added successfully
    /// * `Ok(false)` - Signature already exists or cannot be added (duplicate signature after completion)
    /// * `Err` - Contract not found or other error
    fn add_signature(
        &mut self,
        contract: &Contract,
        public_key: &ECPoint,
        signature: Vec<u8>,
    ) -> Result<bool, String> {
        let hash = contract.script_hash();

        let item = self.items.get_mut(&hash)
            .ok_or_else(|| format!("Contract not found: {}", hash))?;

        // Reject duplicate signatures after context is complete
        if item.is_completed() && item.signatures.contains_key(public_key) {
            return Ok(false);
        }

        Ok(item.add_signature(public_key.clone(), signature))
    }

    /// Adds a contract to the context, preparing it to receive signatures.
    ///
    /// This registers the contract's verification script and parameter requirements.
    fn add_contract(&mut self, contract: Contract) {
        let hash = contract.script_hash();
        let parameter_count = contract.parameter_list.len();

        if !self.items.contains_key(&hash) {
            let item = ContextItem::new(
                contract.script.clone(),
                parameter_count,
            );
            self.items.insert(hash, item);
        }
    }

    /// Checks if all required signatures have been collected for all signers.
    fn completed(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }

        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));

        // Check that all required script hashes have complete signatures
        for hash in &script_hashes {
            match self.items.get(hash) {
                Some(item) if item.is_completed() => continue,
                _ => return false,
            }
        }

        true
    }

    /// Builds and returns the witnesses for the transaction.
    ///
    /// Witnesses are returned in the same order as the transaction's script hashes.
    /// Each witness contains the collected signatures (invocation script) and
    /// the verification script.
    fn witnesses(&self) -> Vec<Witness> {
        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));
        let mut witnesses = Vec::with_capacity(script_hashes.len());

        for hash in &script_hashes {
            if let Some(item) = self.items.get(hash) {
                // Extract public keys from the context for proper ordering
                let public_keys: Vec<ECPoint> = item.signatures.keys().cloned().collect();
                witnesses.push(item.build_witness(&public_keys));
            } else {
                // If no item exists, create an empty witness
                witnesses.push(Witness::new());
            }
        }

        witnesses
    }
}

/// Sign item for transaction signing
#[derive(Clone)]
struct SignItem {
    /// The contract for signing
    contract: Contract,
    /// The key pairs for signing
    key_pairs: Vec<KeyPair>,
    /// Cached public keys for signature ordering
    public_keys: Vec<ECPoint>,
}

impl SignItem {
    /// Creates a new sign item with public keys extracted from key pairs.
    fn new(contract: Contract, key_pairs: Vec<KeyPair>) -> Result<Self, RpcError> {
        let public_keys = key_pairs
            .iter()
            .map(|kp| kp.get_public_key_point())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            contract,
            key_pairs,
            public_keys,
        })
    }

    /// Adds a key pair to this sign item if it's not already present.
    fn add_key(&mut self, key: KeyPair) -> Result<(), RpcError> {
        let exists = self.key_pairs
            .iter()
            .any(|candidate| candidate.private_key() == key.private_key());

        if !exists {
            let public_key = key.get_public_key_point()?;
            self.key_pairs.push(key);
            self.public_keys.push(public_key);
        }

        Ok(())
    }
}

/// Transaction manager for building and signing transactions via RPC.
///
/// Provides a fluent API for:
/// - Adding signatures (single-sig and multi-sig)
/// - Managing witnesses
/// - Calculating fees
/// - Verifying sufficient balance
///
/// Matches C# `Neo.SmartContract.TransactionManager`.
pub struct TransactionManager {
    /// The RPC client instance
    _rpc_client: Arc<RpcClient>,

    /// The transaction context to manage the witnesses
    context: ContractParametersContext,

    /// Container for keys that will sign the transaction
    sign_store: Vec<SignItem>,

    /// The transaction managed by this instance
    tx: Transaction,
}

impl TransactionManager {
    /// Creates a new transaction manager.
    ///
    /// # Arguments
    /// * `tx` - The transaction to manage
    /// * `rpc_client` - RPC client for fee calculation and balance checks
    ///
    /// Matches C# `TransactionManager` constructor.
    pub fn new(tx: Transaction, rpc_client: Arc<RpcClient>) -> Self {
        let snapshot = std::sync::Arc::new(neo_storage::persistence::DataCache::new(true));
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

    /// Gets the managed transaction.
    pub const fn tx(&self) -> &Transaction {
        &self.tx
    }

    /// Helper function for one-off transaction creation.
    ///
    /// Creates a transaction manager with the provided script and signers.
    ///
    /// Matches C# `TransactionManager.MakeTransactionAsync`.
    pub async fn make_transaction(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        signers: Option<Vec<Signer>>,
        attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, RpcError> {
        let factory = TransactionManagerFactory::new(rpc_client);
        let signers = signers.unwrap_or_default();
        let attributes = attributes.unwrap_or_default();
        factory
            .make_transaction_with_attributes(script, &signers, &attributes)
            .await
    }

    /// Helper function for one-off transaction creation with explicit system fee.
    ///
    /// Creates a transaction manager and sets the system fee directly instead of
    /// estimating it via RPC.
    ///
    /// Matches C# `TransactionManager.MakeTransactionAsync` with systemFee parameter.
    pub async fn make_transaction_with_fee(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        system_fee: i64,
        signers: Option<Vec<Signer>>,
        attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, RpcError> {
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

    /// Adds a signature using the provided key pair.
    ///
    /// Creates a single-signature contract from the public key and registers
    /// the key for signing.
    ///
    /// Matches C# `TransactionManager.AddSignature`.
    pub fn add_signature(&mut self, key: &KeyPair) -> Result<&mut Self, RpcError> {
        let public_point = key.get_public_key_point()?;
        let contract = Contract::create_signature_contract(public_point);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Adds a multi-signature using a single key pair.
    ///
    /// Creates a multi-sig contract requiring M-of-N signatures and registers
    /// one key for signing.
    ///
    /// Matches C# `TransactionManager.AddMultiSig` with single `KeyPair`.
    pub fn add_multi_sig(
        &mut self,
        key: &KeyPair,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Adds a multi-signature using multiple key pairs.
    ///
    /// Creates a multi-sig contract and registers multiple keys for signing.
    /// Useful when this transaction manager will sign with multiple keys from
    /// the multi-sig set.
    ///
    /// Matches C# `TransactionManager.AddMultiSig` with `KeyPair` array.
    pub fn add_multi_sig_with_keys(
        &mut self,
        keys: Vec<KeyPair>,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);

        for key in keys {
            self.add_sign_item(contract.clone(), key)?;
        }

        Ok(self)
    }

    /// Adds a witness directly without signing (for contract accounts).
    ///
    /// Useful when the witness will be provided by a contract account or when
    /// the verification script is known but no signature is needed from this manager.
    ///
    /// Matches C# `TransactionManager.AddWitness` with `Contract`.
    pub fn add_witness(&mut self, contract: Contract) -> Result<&mut Self, RpcError> {
        self.context.add_contract(contract);
        Ok(self)
    }

    /// Adds a witness by script hash (synchronous, creates stub contract).
    ///
    /// Creates a contract stub with the given script hash. Use this when the
    /// verification script is empty (deployed contract) or will be provided
    /// through other means.
    ///
    /// For non-deployed contracts, use `add_witness_with_hash_async` to fetch
    /// the contract from RPC.
    ///
    /// Matches C# `TransactionManager.AddWitness` with `UInt160`.
    pub fn add_witness_with_hash(&mut self, script_hash: &UInt160) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_with_hash(*script_hash, Vec::new());
        self.add_witness(contract)
    }

    /// Adds a witness by resolving the contract via RPC.
    ///
    /// Fetches the contract state from the blockchain and extracts the parameter
    /// list from the verify method. Required for proper witness construction of
    /// deployed contract accounts.
    ///
    /// Matches C# `TransactionManager.AddWitness` with RPC lookup.
    pub async fn add_witness_with_hash_async(
        &mut self,
        script_hash: &UInt160,
    ) -> Result<&mut Self, RpcError> {
        let contract = self.get_contract_async(script_hash).await?;
        self.add_witness(contract)
    }

    /// Signs the transaction and calculates fees.
    ///
    /// This method:
    /// 1. Calculates the network fee by simulating witness verification
    /// 2. Verifies the sender has sufficient GAS balance
    /// 3. Signs the transaction with all registered keys
    /// 4. Assembles the final witnesses
    ///
    /// # Returns
    /// The signed transaction with all witnesses attached.
    ///
    /// # Errors
    /// - Insufficient GAS balance
    /// - Missing signatures
    /// - RPC errors during fee calculation or balance check
    ///
    /// Matches C# `TransactionManager.SignAsync`.
    pub async fn sign(&mut self) -> Result<Transaction, RpcError> {
        // Calculate network fee by simulating with empty invocation scripts
        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));
        let mut witnesses = Vec::with_capacity(script_hashes.len());
        for hash in &script_hashes {
            let verification_script = self.get_verification_script(hash);
            witnesses.push(Witness::new_with_scripts(Vec::new(), verification_script));
        }
        self.tx.set_witnesses(witnesses);

        let network_fee = self._rpc_client.calculate_network_fee(&self.tx).await?;
        self.tx.set_network_fee(network_fee);

        // Clear temporary witnesses before signing
        self.tx.set_witnesses(Vec::new());

        // Verify sufficient GAS balance
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

        // Sign with all registered keys
        let sign_data = get_sign_data_vec(&self.tx, self._rpc_client.protocol_settings.network)?;
        for item in &self.sign_store {
            for (key, public_key) in item.key_pairs.iter().zip(&item.public_keys) {
                let signature = key.sign(&sign_data)?;
                let added = self
                    .context
                    .add_signature(&item.contract, public_key, signature)
                    .map_err(|err| format!("AddSignature failed: {err}"))?;
                if !added {
                    return Err("AddSignature failed!".into());
                }
            }
        }

        // Verify all signatures collected
        if !self.context.completed() {
            return Err("Please add signature or witness first!".into());
        }

        // Build final witnesses from collected signatures
        let final_witnesses = self.context.witnesses();
        if final_witnesses.is_empty() {
            return Err("No witnesses available; context incomplete".to_string().into());
        }
        self.tx.set_witnesses(final_witnesses);

        Ok(self.tx.clone())
    }

    // ==================== Private Helper Methods ====================

    /// Adds a signing item to the internal store.
    ///
    /// Verifies the contract's script hash matches one of the transaction's signers,
    /// then registers the key for signing. If the contract is already registered,
    /// adds the key to the existing item.
    fn add_sign_item(&mut self, contract: Contract, key: KeyPair) -> Result<(), RpcError> {
        let hash = contract.script_hash();
        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));
        if !script_hashes.contains(&hash) {
            return Err(format!("Add SignItem error: Mismatch ScriptHash ({hash})").into());
        }

        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|i| i.contract.script_hash() == hash)
        {
            item.add_key(key)?;
        } else {
            self.sign_store.push(SignItem::new(contract.clone(), vec![key])?);
        }

        self.context.add_contract(contract);
        Ok(())
    }

    /// Retrieves the verification script for a given script hash.
    ///
    /// Searches the sign store for a contract matching the script hash and
    /// returns its verification script. Returns empty vec if not found.
    fn get_verification_script(&self, hash: &UInt160) -> Vec<u8> {
        for item in &self.sign_store {
            if item.contract.script_hash() == *hash {
                return item.contract.script.clone();
            }
        }
        Vec::new()
    }

    /// Fetches a contract from the blockchain via RPC.
    ///
    /// Retrieves the contract state and extracts the parameter list from the
    /// `verify` method. This is required for proper witness construction of
    /// deployed contracts.
    async fn get_contract_async(&self, script_hash: &UInt160) -> Result<Contract, RpcError> {
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
    use neo_protocol::ProtocolSettings;
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
    async fn make_transaction_preserves_attributes() {
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
        let manager = TransactionManager::make_transaction(
            client,
            &[0x01],
            None,
            Some(vec![TransactionAttribute::HighPriority]),
        )
        .await
        .expect("manager");

        assert_eq!(manager.tx().attributes().len(), 1);
        assert!(matches!(
            manager.tx().attributes()[0],
            TransactionAttribute::HighPriority
        ));
    }

    #[tokio::test]
    async fn make_transaction_without_attributes_uses_empty_list() {
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
        let manager = TransactionManager::make_transaction(client, &[0x01], None, None)
            .await
            .expect("manager");

        assert!(manager.tx().attributes().is_empty());
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
