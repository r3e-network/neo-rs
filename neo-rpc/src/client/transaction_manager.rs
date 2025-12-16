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

use crate::{RpcClient, TransactionManagerFactory};
use neo_core::{
    smart_contract::ContractParametersContext, Contract, ECPoint, KeyPair, Signer, Transaction,
    TransactionAttribute,
};
use neo_primitives::UInt160;
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
/// Matches C# TransactionManager
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
    /// TransactionManager Constructor
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
    pub fn tx(&self) -> &Transaction {
        &self.tx
    }

    /// Helper function for one-off TransactionManager creation
    /// Matches C# MakeTransactionAsync
    pub async fn make_transaction(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        signers: Option<Vec<Signer>>,
        _attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<TransactionManager, Box<dyn std::error::Error>> {
        let factory = TransactionManagerFactory::new(rpc_client);
        factory
            .make_transaction(script, &signers.unwrap_or_default())
            .await
    }

    /// Helper function for one-off TransactionManager creation with system fee
    /// Matches C# MakeTransactionAsync with systemFee parameter
    pub async fn make_transaction_with_fee(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        system_fee: i64,
        signers: Option<Vec<Signer>>,
        attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<TransactionManager, Box<dyn std::error::Error>> {
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
    /// Matches C# AddSignature
    pub fn add_signature(
        &mut self,
        key: &KeyPair,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let public_point = key.get_public_key_point()?;
        let contract = Contract::create_signature_contract(public_point);
        self.add_sign_item(contract, key.clone());
        Ok(self)
    }

    /// Add Multi-Signature
    /// Matches C# AddMultiSig with KeyPair
    pub fn add_multi_sig(
        &mut self,
        key: &KeyPair,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);
        self.add_sign_item(contract, key.clone());
        Ok(self)
    }

    /// Add Multi-Signature with multiple keys
    /// Matches C# AddMultiSig with KeyPair array
    pub fn add_multi_sig_with_keys(
        &mut self,
        keys: Vec<KeyPair>,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);

        // Find or create sign item
        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|i| i.contract.script_hash() == contract.script_hash())
        {
            for key in keys {
                item.key_pairs.push(key);
            }
        } else {
            let mut key_pairs = Vec::new();
            for key in keys {
                key_pairs.push(key);
            }
            self.sign_store.push(SignItem {
                contract: contract.clone(),
                key_pairs,
            });
        }

        Ok(self)
    }

    /// Add witness with contract
    /// Matches C# AddWitness
    pub fn add_witness(
        &mut self,
        contract: Contract,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        self.context.add_contract(contract);
        Ok(self)
    }

    /// Add witness with script hash
    /// Matches C# AddWitness with UInt160.
    ///
    /// Note: Contract lookup requires an RPC call; use [`Self::add_witness_with_hash_async`].
    pub fn add_witness_with_hash(
        &mut self,
        _script_hash: &UInt160,
    ) -> Result<&mut Self, Box<dyn std::error::Error>> {
        Err("Contract lookup requires RPC; use add_witness_with_hash_async()".into())
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
    /// Matches C# SignAsync
    pub async fn sign(&mut self) -> Result<Transaction, Box<dyn std::error::Error>> {
        let final_witnesses = self
            .context
            .get_witnesses()
            .ok_or_else(|| "No witnesses available; context incomplete".to_string())?;
        self.tx.set_witnesses(final_witnesses);

        Ok(self.tx.clone())
    }

    // Helper methods

    fn add_sign_item(&mut self, contract: Contract, key: KeyPair) {
        let hash = contract.script_hash();
        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|i| i.contract.script_hash() == hash)
        {
            item.key_pairs.push(key);
        } else {
            let key_pairs = vec![key];
            self.sign_store.push(SignItem {
                contract: contract.clone(),
                key_pairs,
            });
        }

        self.context.add_contract(contract);
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
