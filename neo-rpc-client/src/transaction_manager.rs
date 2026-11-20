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
use neo_crypto::ECPoint;
use neo_native_contracts::native::{GasToken, NativeContract};
use neo_primitives::UInt160;
use neo_protocol::payloads::{Signer, Transaction, TransactionAttribute};
use neo_smart_contract::contract::Contract;
use neo_wallets::helper::Helper as WalletHelper;
use neo_wallets::{
    AccountSignature, AuthorizationLimits, KeyPair, SignerRequest, TransactionAuthorizationError,
    TransactionAuthorizer, TransactionChainQuery, TransactionSigner,
};
use num_bigint::BigInt;
use std::sync::Arc;

/// Remote chain queries consulted while authorizing a transaction.
///
/// This is the only chain-facing piece of the client: fee estimation and the
/// sender's GAS balance. Signer resolution, witness ordering, completion and
/// funding rules live in [`TransactionAuthorizer`], which `neo-rpc-server`
/// drives the same way from its local ledger.
struct RpcChainQuery {
    /// Connection used for `calculatenetworkfee` and `invokescript`.
    client: Arc<RpcClient>,
}

#[async_trait::async_trait]
impl TransactionChainQuery for RpcChainQuery {
    type Error = RpcError;

    async fn block_height(&self) -> Result<u32, Self::Error> {
        // `getblockcount` reports height + 1.
        Ok(self.client.get_block_count().await?.saturating_sub(1))
    }

    async fn gas_balance(&self, account: &UInt160) -> Result<BigInt, Self::Error> {
        Nep17Api::new(Arc::clone(&self.client))
            .balance_of(&GasToken::new().hash(), account)
            .await
    }

    async fn network_fee(&self, transaction: &Transaction) -> Result<i64, Self::Error> {
        Ok(self.client.calculate_network_fee(transaction).await?)
    }
}

/// Maps an authorization failure onto the client error type.
///
/// The two historical client messages are preserved so callers keep the same
/// diagnostics they had before authorization moved into `neo-wallets`.
fn authorization_error(error: TransactionAuthorizationError) -> RpcError {
    match error {
        TransactionAuthorizationError::Incomplete { .. } => {
            "Please add signature or witness first!".into()
        }
        TransactionAuthorizationError::UnknownSigner(account) => {
            format!("Add SignItem error: Mismatch ScriptHash ({account})").into()
        }
        other => other.to_string().into(),
    }
}

/// Signs on behalf of the manager with the keys registered in its store.
///
/// The contract's verification script decides each signature's slot, so an
/// account the manager holds no key for simply declines to sign and leaves its
/// witness to the caller.
struct KeyStoreSigner<'a> {
    /// Contracts and keys registered through `add_signature`/`add_multi_sig`.
    sign_store: &'a [SignItem],
}

#[async_trait::async_trait]
impl TransactionSigner for KeyStoreSigner<'_> {
    async fn sign(
        &self,
        request: &SignerRequest<'_>,
    ) -> Result<Vec<AccountSignature>, TransactionAuthorizationError> {
        let Some(item) = self
            .sign_store
            .iter()
            .find(|item| item.contract.script_hash() == *request.account)
        else {
            return Ok(Vec::new());
        };
        let mut signatures = Vec::with_capacity(item.key_pairs.len());
        for key in &item.key_pairs {
            let signature = key.sign(&request.sign_data).map_err(|err| {
                TransactionAuthorizationError::SigningFailed(format!("AddSignature failed: {err}"))
            })?;
            signatures.push(AccountSignature::new(
                key.compressed_public_key(),
                signature,
            ));
        }
        Ok(signatures)
    }
}

/// Sign item for transaction signing
#[derive(Clone)]
struct SignItem {
    /// The contract for signing
    contract: Contract,
    /// The key pairs for signing
    key_pairs: Vec<KeyPair>,
}

impl SignItem {
    /// Creates a new sign item.
    fn new(contract: Contract, key_pairs: Vec<KeyPair>) -> Self {
        Self {
            contract,
            key_pairs,
        }
    }

    /// Adds a key pair to this sign item if it's not already present.
    fn add_key(&mut self, key: KeyPair) {
        let exists = self
            .key_pairs
            .iter()
            .any(|candidate| candidate.private_key() == key.private_key());

        if !exists {
            self.key_pairs.push(key);
        }
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
    rpc_client: Arc<RpcClient>,

    /// Transaction authorization shared with `neo-rpc-server`
    authorizer: TransactionAuthorizer,

    /// Container for keys that will sign the transaction
    sign_store: Vec<SignItem>,
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
        let network = rpc_client.protocol_settings.network;
        let limits =
            AuthorizationLimits::new(rpc_client.protocol_settings.max_valid_until_block_increment);
        Self {
            authorizer: TransactionAuthorizer::new(tx, network, limits),
            rpc_client,
            sign_store: Vec::new(),
        }
    }

    /// Gets the managed transaction.
    pub const fn tx(&self) -> &Transaction {
        self.authorizer.tx()
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
        manager.authorizer.set_system_fee(system_fee);

        if let Some(attrs) = attributes {
            manager.authorizer.set_attributes(attrs);
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
        self.authorizer
            .register_contract(&contract)
            .map_err(authorization_error)?;
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
    /// 1. Validates the transaction shape and fee fields
    /// 2. Calculates the network fee by simulating witness verification
    /// 3. Verifies the sender has sufficient GAS balance
    /// 4. Signs the transaction with all registered keys
    /// 5. Assembles the final witnesses in transaction signer order
    ///
    /// Signer resolution, witness ordering, completion and funding rules are
    /// owned by [`TransactionAuthorizer`]; this method only supplies the RPC
    /// chain port and the key material registered on the manager.
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
        self.authorizer
            .validate_fees()
            .map_err(authorization_error)?;

        let chain = RpcChainQuery {
            client: Arc::clone(&self.rpc_client),
        };

        // The node prices verification from the verification script, so the
        // estimate runs against placeholder witnesses that the authorizer
        // installs and removes again.
        self.authorizer
            .estimate_network_fee(&chain)
            .await
            .map_err(authorization_error)?;

        if let Err(error) = self.authorizer.ensure_sufficient_funds(&chain).await {
            if matches!(
                error,
                TransactionAuthorizationError::InsufficientFunds { .. }
            ) {
                let address = self
                    .authorizer
                    .tx()
                    .sender()
                    .map(|sender| {
                        WalletHelper::to_address(
                            &sender,
                            self.rpc_client.protocol_settings.address_version,
                        )
                    })
                    .unwrap_or_default();
                return Err(format!("Insufficient GAS in address: {address}").into());
            }
            return Err(authorization_error(error));
        }

        let signer = KeyStoreSigner {
            sign_store: &self.sign_store,
        };
        self.authorizer
            .collect_signatures(&signer)
            .await
            .map_err(authorization_error)?;

        self.authorizer.finalize().map_err(authorization_error)
    }

    // ==================== Private Helper Methods ====================

    /// Adds a signing item to the internal store.
    ///
    /// Registers the contract with the shared authorizer, which rejects a
    /// contract whose script hash is not one of the transaction signers, then
    /// records the key so `sign` can use it.
    fn add_sign_item(&mut self, contract: Contract, key: KeyPair) -> Result<(), RpcError> {
        self.authorizer
            .register_contract(&contract)
            .map_err(authorization_error)?;

        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|item| item.contract.script_hash() == contract.script_hash())
        {
            item.add_key(key);
        } else {
            self.sign_store.push(SignItem::new(contract, vec![key]));
        }
        Ok(())
    }

    /// Fetches a contract from the blockchain via RPC.
    ///
    /// Retrieves the contract state and extracts the parameter list from the
    /// `verify` method. This is required for proper witness construction of
    /// deployed contracts.
    async fn get_contract_async(&self, script_hash: &UInt160) -> Result<Contract, RpcError> {
        let state = self
            .rpc_client
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
    use neo_json::{JArray, JObject, JToken};
    use neo_primitives::WitnessScope;
    use neo_protocol::ProtocolSettings;
    use neo_protocol::signing_data::get_sign_data_vec;
    use neo_smart_contract::helper::Helper as ContractHelper;
    use neo_smart_contract::witness_verification::verify_witness_multi_signature;
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
    async fn sign_multi_sig_witness_follows_verification_script_order() {
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
        let keys: Vec<KeyPair> = [5u8, 6u8, 7u8]
            .into_iter()
            .map(|seed| KeyPair::from_private_key(&[seed; 32]).expect("key"))
            .collect();
        let points: Vec<ECPoint> = keys
            .iter()
            .map(|key| key.get_public_key_point().expect("point"))
            .collect();
        // 2-of-3 contract: only two of the three script keys sign.
        let contract = Contract::create_multi_sig_contract(2, &points);
        let signers = vec![Signer::new(contract.script_hash(), WitnessScope::GLOBAL)];

        let mut manager =
            TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
                .await
                .expect("manager");
        // Register the signers in an order unrelated to the verification script.
        manager
            .add_multi_sig(&keys[2], 2, points.clone())
            .expect("add multisig signer c");
        manager
            .add_multi_sig(&keys[0], 2, points)
            .expect("add multisig signer a");
        let tx = manager.sign().await.expect("sign");

        let witness = &tx.witnesses()[0];
        assert_eq!(witness.verification_script(), contract.script.as_slice());
        let signatures: Vec<Vec<u8>> = witness
            .invocation_script()
            .chunks(66)
            .map(|chunk| chunk[2..].to_vec())
            .collect();
        assert_eq!(
            signatures.len(),
            2,
            "a 2-of-3 witness pushes exactly two signatures"
        );

        let sign_data =
            get_sign_data_vec(&tx, client.protocol_settings.network).expect("sign data");
        let (_, script_keys) =
            ContractHelper::parse_multi_sig_contract(&contract.script).expect("canonical script");
        assert!(
            verify_witness_multi_signature(
                &sign_data,
                &contract.script_hash(),
                2,
                &script_keys,
                &signatures
            )
            .expect("verifier must not fault"),
            "signatures must line up with the verification-script key order"
        );
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
